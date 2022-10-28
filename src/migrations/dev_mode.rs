use edgedb_client::client::Connection;
use linked_hash_map::LinkedHashMap;

use anyhow::Context as _;

use crate::commands::parser::CreateMigration;
use crate::commands::parser::Migrate;
use crate::commands::Options;
use crate::migrations::context::Context;
use crate::migrations::create::{CurrentMigration, normal_migration};
use crate::migrations::create::{execute, query_row, execute_start_migration};
use crate::migrations::migrate::{apply_migrations, apply_migrations_inner};
use crate::migrations::migration::{self, MigrationFile};
use crate::migrations::timeout;

enum Mode {
    Normal { skip: usize },
    Rebase,
}

pub async fn migrate(cli: &mut Connection, ctx: Context, migrate: &Migrate)
    -> anyhow::Result<()>
{
    let mut migrations = migration::read_all(&ctx, true).await?;
    let db_migration = get_db_migration(cli).await?;
    match select_mode(cli, &migrations, db_migration.as_deref()).await? {
        Mode::Normal { skip } => {
            log::info!("Skipping {} revisions.", skip);
            for _ in 0..skip {
                migrations.pop_front();
            }
            if !migrations.is_empty() {
                apply_migrations(cli, &migrations, migrate).await?;
            }
            log::info!("Calculating schema diff.");
            migrate_to_schema(cli, &ctx).await?;
        }
        Mode::Rebase => {
            log::info!("Calculating schema diff.");
            migrate_to_schema(cli, &ctx).await?;
            log::info!("Now rebasing on top of filesystem migrations.");
            rebase_to_schema(cli, &ctx, &migrations).await?;
        }
    }
    Ok(())
}

async fn select_mode(cli: &mut Connection,
                     migrations: &LinkedHashMap<String, MigrationFile>,
                     db_migration: Option<&str>)
    -> anyhow::Result<Mode>
{
    if let Some(db_migration) = &db_migration {
        for (idx, (key, _)) in migrations.iter().enumerate() {
            if key == db_migration {
                return Ok(Mode::Normal { skip: idx+1 });
            }
        }
        let last_fs_migration = migrations.back().map(|(id, _)| id.clone());
        if let Some(id) = last_fs_migration {
            let contains_last_fs_migration: bool = cli.query_row(r###"
                    select exists(
                        select schema::Migration filter .name = <str>$0
                    )
                "###, &(id,)).await?;
            if contains_last_fs_migration {
                Ok(Mode::Normal { skip: migrations.len() })
            } else {
                Ok(Mode::Rebase)
            }
        } else {
            Ok(Mode::Normal { skip: migrations.len() /* == 0 */ })
        }
    } else {
        Ok(Mode::Normal { skip: 0 })
    }
}

async fn get_db_migration(cli: &mut Connection)
    -> anyhow::Result<Option<String>>
{
    let res = cli.query_row_opt(r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := Last.name
        "###, &()).await?;
    Ok(res)
}

async fn migrate_to_schema(cli: &mut Connection, ctx: &Context)
    -> anyhow::Result<()>
{
    execute_start_migration(&ctx, cli).await?;
    execute(cli, "POPULATE MIGRATION").await?;
    let descr = query_row::<CurrentMigration>(cli,
        "DESCRIBE CURRENT MIGRATION AS JSON"
    ).await?;
    if !descr.complete {
        // TODO(tailhook) is `POPULATE MIGRATION` equivalent to `--yolo` or
        // should we do something manually?
        anyhow::bail!("Migration cannot be automatically populated");
    }
    execute(cli, "ABORT MIGRATION").await?;
    if !descr.confirmed.is_empty() {
        execute(cli, format!(
        "CREATE MIGRATION {{
            SET generated_by := schema::MigrationGeneratedBy.DevMode;
            {}
        }}", descr.confirmed.join("\n"))).await?;
    }
    Ok(())
}

async fn rebase_to_schema(cli: &mut Connection, ctx: &Context,
                          migrations: &LinkedHashMap<String, MigrationFile>)
    -> anyhow::Result<()>
{
    execute(cli, "START MIGRATION REWRITE").await?;

    let res = async {
        apply_migrations_inner(cli, migrations, true).await?;
        execute_start_migration(&ctx, cli).await?;
        execute(cli, "POPULATE MIGRATION").await?;
        let descr = query_row::<CurrentMigration>(cli,
            "DESCRIBE CURRENT MIGRATION AS JSON"
        ).await?;
        if !descr.complete {
            // TODO(tailhook) is `POPULATE MIGRATION` equivalent to `--yolo` or
            // should we do something manually?
            anyhow::bail!("Migration cannot be automatically populated");
        }
        execute(cli, "ABORT MIGRATION").await?;
        if !descr.confirmed.is_empty() {
            execute(cli, format!(
            "CREATE MIGRATION {{
                SET generated_by := schema::MigrationGeneratedBy.DevMode;
                {}
            }}", descr.confirmed.join("\n"))).await?;
        }
        Ok(())
    }.await;

    match res {
        Ok(()) => {
            execute(cli, "COMMIT MIGRATION REWRITE").await
                .context("commit migration rewrite")?;
            Ok(())
        }
        Err(e) => {
            execute(cli, "ABORT MIGRATION REWRITE").await
                .map_err(|e| {
                    log::warn!("Error aborting migration rewrite: {:#}", e);
                }).ok();
            Err(e)
        }
    }
}

async fn create_in_rewrite(ctx: &Context, cli: &mut Connection,
                           migrations: &LinkedHashMap<String, MigrationFile>,
                           create: &CreateMigration)
    -> anyhow::Result<()>
{
    apply_migrations_inner(cli, migrations, true).await?;
    normal_migration(cli, ctx, migrations, create).await?;
    Ok(())
}

pub async fn create(cli: &mut Connection, ctx: &Context, _options: &Options,
    create: &CreateMigration)
    -> anyhow::Result<()>
{
    let migrations = migration::read_all(&ctx, true).await?;

    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    execute(cli, "START MIGRATION REWRITE").await?;

    let res = create_in_rewrite(ctx, cli, &migrations, create).await;
    let drop_res = if cli.is_consistent() {
        execute(cli, "ABORT MIGRATION REWRITE").await
            .context("migration rewrite cleanup")
    } else {
        Ok(())
    };
    let timeo_res = if cli.is_consistent() {
        timeout::restore_for_transaction(cli, old_timeout).await
    } else {
        Ok(())
    };
    res.and(drop_res).and(timeo_res)
}
