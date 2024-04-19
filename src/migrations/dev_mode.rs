use crate::connect::Connection;
use indexmap::IndexMap;

use anyhow::Context as _;
use edgedb_errors::QueryError;
use indicatif::ProgressBar;
use once_cell::sync::Lazy;

use crate::async_try;
use crate::bug;
use crate::commands::Options;
use crate::migrations::context::Context;
use crate::migrations::create::{execute_start_migration, unsafe_populate};
use crate::migrations::create::{first_migration, normal_migration};
use crate::migrations::create::{write_migration, MigrationKey};
use crate::migrations::create::{CurrentMigration, FutureMigration};
use crate::migrations::edb::{execute, execute_if_connected, query_row};
use crate::migrations::migrate::{apply_migrations, apply_migrations_inner};
use crate::migrations::migration::{self, MigrationFile};
use crate::migrations::options::CreateMigration;
use crate::migrations::timeout;
use crate::portable::ver;

enum Mode {
    Normal { skip: usize },
    Rebase,
}

static MINIMUM_VERSION: Lazy<ver::Build> = Lazy::new(|| "3.0-alpha.1+05474ea".parse().unwrap());

mod ddl {
    // Just for nice log filter
    use super::{execute, Connection};

    pub async fn apply_statements(cli: &mut Connection, items: &[String]) -> anyhow::Result<()> {
        execute(
            cli,
            format!(
                "CREATE MIGRATION {{
                SET generated_by := schema::MigrationGeneratedBy.DevMode;
                {}
            }}",
                items.join("\n")
            ),
        )
        .await?;
        for ddl_statement in items {
            log::info!("{}", ddl_statement);
        }
        Ok(())
    }
}

pub async fn check_client(cli: &mut Connection) -> anyhow::Result<bool> {
    ver::check_client(cli, &MINIMUM_VERSION).await
}

pub async fn migrate(cli: &mut Connection, ctx: &Context, bar: &ProgressBar) -> anyhow::Result<()> {
    if !check_client(cli).await? {
        anyhow::bail!(
            "Dev mode is not supported on EdgeDB {}. Please upgrade.",
            cli.get_version().await?
        );
    }
    let migrations = migration::read_all(ctx, true).await?;
    let db_migration = get_db_migration(cli).await?;
    match select_mode(cli, &migrations, db_migration.as_deref()).await? {
        Mode::Normal { skip } => {
            log::info!("Skipping {} revisions.", skip);
            let migrations = migrations
                .get_range(skip..)
                .ok_or_else(|| bug::error("`skip` is out of range"))?;
            if !migrations.is_empty() {
                bar.set_message("applying migrations");
                apply_migrations(cli, migrations, ctx, false).await?;
            }
            bar.set_message("calculating diff");
            log::info!("Calculating schema diff.");
            migrate_to_schema(cli, ctx).await?;
        }
        Mode::Rebase => {
            log::info!("Calculating schema diff.");
            bar.set_message("calculating diff");
            migrate_to_schema(cli, ctx).await?;
            log::info!("Now rebasing on top of filesystem migrations.");
            bar.set_message("rebasing migrations");
            rebase_to_schema(cli, ctx, &migrations).await?;
        }
    }
    Ok(())
}

async fn select_mode(
    cli: &mut Connection,
    migrations: &IndexMap<String, MigrationFile>,
    db_migration: Option<&str>,
) -> anyhow::Result<Mode> {
    if let Some(db_migration) = &db_migration {
        for (idx, (key, _)) in migrations.iter().enumerate() {
            if key == db_migration {
                return Ok(Mode::Normal { skip: idx + 1 });
            }
        }
        let last_fs_migration = migrations.last().map(|(id, _)| id.clone());
        if let Some(id) = last_fs_migration {
            let contains_last_fs_migration: bool = cli
                .query_required_single(
                    r###"
                    select exists(
                        select schema::Migration filter .name = <str>$0
                    )
                "###,
                    &(id,),
                )
                .await?;
            if contains_last_fs_migration {
                Ok(Mode::Normal {
                    skip: migrations.len(),
                })
            } else {
                Ok(Mode::Rebase)
            }
        } else {
            Ok(Mode::Normal {
                skip: migrations.len(), /* == 0 */
            })
        }
    } else {
        Ok(Mode::Normal { skip: 0 })
    }
}

async fn get_db_migration(cli: &mut Connection) -> anyhow::Result<Option<String>> {
    let res = cli
        .query_single(
            r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := assert_single(Last.name)
        "###,
            &(),
        )
        .await?;
    Ok(res)
}

async fn migrate_to_schema(cli: &mut Connection, ctx: &Context) -> anyhow::Result<()> {
    use edgedb_protocol::server_message::TransactionState::NotInTransaction;

    let transaction = matches!(cli.transaction_state(), NotInTransaction);
    if transaction {
        execute(cli, "START TRANSACTION").await?;
    }
    async_try! {
        async {
            _migrate_to_schema(cli, ctx).await
        },
        finally async {
            if transaction {
                execute_if_connected(cli, "COMMIT").await?;
            }
            anyhow::Ok(())
        }
    }
}

async fn _migrate_to_schema(cli: &mut Connection, ctx: &Context) -> anyhow::Result<()> {
    execute(cli, "DECLARE SAVEPOINT migrate_to_schema").await?;
    let descr = async_try! {
        async {
            execute_start_migration(ctx, cli).await?;
            if let Err(e) = execute(cli, "POPULATE MIGRATION").await {
                if e.is::<QueryError>() {
                    return Ok(None)
                } else {
                    return Err(e)?;
                }
            }
            let descr = query_row::<CurrentMigration>(cli,
                "DESCRIBE CURRENT MIGRATION AS JSON"
            ).await?;
            if !descr.complete {
                anyhow::bail!("Migration cannot be automatically populated");
            }
            Ok(Some(descr))
        },
        finally async {
            execute_if_connected(cli,
                "ROLLBACK TO SAVEPOINT migrate_to_schema",
            ).await?;
            execute_if_connected(cli,
                "RELEASE SAVEPOINT migrate_to_schema",
            ).await
        }
    }?;
    let descr = if let Some(descr) = descr {
        descr
    } else {
        execute_start_migration(ctx, cli).await?;
        async_try! {
            async {
                unsafe_populate(ctx, cli).await
            },
            finally async {
                execute_if_connected(cli, "ABORT MIGRATION",).await
            }
        }?
    };
    if !descr.confirmed.is_empty() {
        ddl::apply_statements(cli, &descr.confirmed).await?;
    }
    Ok(())
}

pub async fn rebase_to_schema(
    cli: &mut Connection,
    ctx: &Context,
    migrations: &IndexMap<String, MigrationFile>,
) -> anyhow::Result<()> {
    execute(cli, "START MIGRATION REWRITE").await?;

    let res = async {
        apply_migrations_inner(cli, migrations, true).await?;
        migrate_to_schema(cli, ctx).await?;
        Ok(())
    }
    .await;

    match res {
        Ok(()) => {
            execute(cli, "COMMIT MIGRATION REWRITE")
                .await
                .context("commit migration rewrite")?;
            Ok(())
        }
        Err(e) => {
            execute_if_connected(cli, "ABORT MIGRATION REWRITE")
                .await
                .map_err(|e| {
                    log::warn!("Error aborting migration rewrite: {:#}", e);
                })
                .ok();
            Err(e)
        }
    }
}

async fn create_in_rewrite(
    ctx: &Context,
    cli: &mut Connection,
    migrations: &IndexMap<String, MigrationFile>,
    create: &CreateMigration,
) -> anyhow::Result<FutureMigration> {
    apply_migrations_inner(cli, migrations, true).await?;
    if migrations.is_empty() {
        first_migration(cli, ctx, create).await
    } else {
        let key = MigrationKey::Index((migrations.len() + 1) as u64);
        let parent = migrations.keys().last().map(|x| &x[..]);
        normal_migration(cli, ctx, key, parent, create).await
    }
}

pub async fn create(
    cli: &mut Connection,
    ctx: &Context,
    _options: &Options,
    create: &CreateMigration,
) -> anyhow::Result<()> {
    let migrations = migration::read_all(ctx, true).await?;

    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    let migration = async_try! {
        async {
            execute(cli, "START MIGRATION REWRITE").await?;
            async_try! {
                async {
                    create_in_rewrite(ctx, cli, &migrations, create).await
                },
                finally async {
                    execute_if_connected(cli, "ABORT MIGRATION REWRITE").await
                        .context("migration rewrite cleanup")
                }
            }
        },
        finally async {
            timeout::restore_for_transaction(cli, old_timeout).await
        }
    }?;
    write_migration(ctx, &migration, !create.non_interactive).await?;
    Ok(())
}
