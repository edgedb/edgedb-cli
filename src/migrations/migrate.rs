use anyhow::Context as _;
use async_std::fs;
use async_std::path::Path;
use edgedb_protocol::value::Value;
use linked_hash_map::LinkedHashMap;

use crate::commands::Options;
use crate::commands::parser::Migrate;
use crate::client::Connection;
use crate::migrations::context::Context;
use crate::migrations::migration::{self, MigrationFile};


fn skip_revisions(migrations: &mut LinkedHashMap<String, MigrationFile>,
    db_migration: &str)
    -> anyhow::Result<()>
{
    while let Some((key, value)) = migrations.pop_front() {
        if key == db_migration {
            return Ok(())
        }
    }
    anyhow::bail!("There is no database revision {} \
        in the filesystem. Consider updating sources.",
        db_migration);
}

pub async fn migrate(cli: &mut Connection, options: &Options,
    migrate: &Migrate)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_config(&migrate.cfg);
    let mut migrations = migration::read_all(&ctx, true).await?;
    let db_migration: Option<String> = cli.query_row_opt(r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := Last.name
        "###, &Value::empty_tuple()).await?;
    if let Some(db_migration) = &db_migration {
        skip_revisions(&mut migrations, db_migration)?;
    };
    if migrations.is_empty() {
        if !migrate.quiet {
            eprintln!("Everything is up to date. Revision {:?}",
                db_migration.as_ref().map(|x| &x[..]).unwrap_or("initial"));
        }
        return Ok(());
    }
    for (_, migration) in migrations {
        let data = fs::read_to_string(&migration.path).await
            .context("error re-reading migration file")?;
        cli.execute(data).await?;
        if !migrate.quiet {
            eprintln!("Applied {}({})",
                migration.data.id,
                Path::new(migration.path.file_name().unwrap()).display());
        }
    }
    return Ok(())
}
