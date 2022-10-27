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

pub async fn migrate(cli: &mut Connection, ctx: Context, migrate: &Migrate)
    -> anyhow::Result<()>
{
    apply_fs(cli, &ctx, migrate).await?;
    migrate_to_schema(cli, &ctx).await?;
    Ok(())
}

async fn skip_and_check_revisions(cli: &mut Connection,
    migrations: &mut LinkedHashMap<String, MigrationFile>,
    db_migration: &str)
    -> anyhow::Result<()>
{
    let last_fs_migration = migrations.back().map(|(id, _)| id.clone());
    while let Some((key, _)) = migrations.pop_front() {
        if key == db_migration {
            return Ok(())
        }
    }
    if let Some(id) = last_fs_migration {
        let contains_last_fs_migration: bool = cli.query_row(r###"
                select exists(
                    select schema::Migration filter .name = <str>$0
                )
            "###, &(id,)).await?;
        if !contains_last_fs_migration {
            // TODO(tailhook) do the rebase
            log::warn!("Migrations in the database and \
                        the filesystem are diverging.");
        }
    }
    Ok(())
}

async fn apply_fs(cli: &mut Connection, ctx: &Context, migrate: &Migrate)
    -> anyhow::Result<()>
{
    let mut migrations = migration::read_all(&ctx, true).await?;
    let db_migration: Option<String> = cli.query_row_opt(r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := Last.name
        "###, &()).await?;

    if let Some(db_migration) = &db_migration {
        skip_and_check_revisions(cli, &mut migrations, db_migration).await?;
    }
    if !migrations.is_empty() {
        apply_migrations(cli, &migrations, migrate).await?;
    }
    Ok(())
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

async fn create_in_rewrite(ctx: &Context, cli: &mut Connection,
                           migrations: &LinkedHashMap<String, MigrationFile>,
                           create: &CreateMigration)
    -> anyhow::Result<()>
{
    apply_migrations_inner(cli, migrations, &Migrate {
        cfg: create.cfg.clone(),
        quiet: true,
        to_revision: None,
        dev_mode: false,
    }).await?;
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
