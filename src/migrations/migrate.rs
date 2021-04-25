use anyhow::Context as _;
use async_std::fs;
use async_std::path::Path;
use async_std::stream::StreamExt;
use edgedb_client::client::Connection;
use edgedb_protocol::value::Value;
use linked_hash_map::LinkedHashMap;

use crate::commands::parser::Migrate;
use crate::commands::ExitCode;
use crate::commands::Options;
use crate::migrations::context::Context;
use crate::migrations::migration::{self, MigrationFile};

fn skip_revisions(
    migrations: &mut LinkedHashMap<String, MigrationFile>,
    db_migration: &str,
) -> anyhow::Result<()> {
    while let Some((key, _)) = migrations.pop_front() {
        if key == db_migration {
            return Ok(());
        }
    }
    anyhow::bail!(
        "There is no database revision {} \
        in the filesystem. Consider updating sources.",
        db_migration
    );
}

async fn check_revision_in_db(
    cli: &mut Connection,
    prefix: &str,
) -> Result<Option<String>, anyhow::Error> {
    let mut items = cli
        .query::<String>(
            r###"
        SELECT name := schema::Migration.name
        FILTER name LIKE <str>$0
        "###,
            &Value::Tuple(vec![Value::Str(format!("{}%", prefix))]),
        )
        .await?;
    let mut all_similar = Vec::new();
    while let Some(name) = items.next().await.transpose()? {
        all_similar.push(name);
    }
    if all_similar.is_empty() {
        return Ok(None);
    }
    if all_similar.len() > 1 {
        anyhow::bail!("More than one revision matches prefix {:?}", prefix);
    }
    return Ok(all_similar.pop());
}

pub async fn migrate(
    cli: &mut Connection,
    _options: &Options,
    migrate: &Migrate,
) -> Result<(), anyhow::Error> {
    let ctx = Context::from_config(&migrate.cfg);

    let mut migrations = migration::read_all(&ctx, true).await?;
    let db_migration: Option<String> = cli
        .query_row_opt(
            r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := Last.name
        "###,
            &Value::empty_tuple(),
        )
        .await?;

    let target_rev = if let Some(prefix) = &migrate.to_revision {
        let db_rev = check_revision_in_db(cli, prefix).await?;
        let file_revs = migrations
            .keys()
            .filter(|r| r.starts_with(prefix))
            .collect::<Vec<_>>();
        if file_revs.len() > 1 {
            anyhow::bail!("More than one revision matches prefix {:?}", prefix);
        }
        let target_rev = match (&db_rev, file_revs.last()) {
            (None, None) => {
                anyhow::bail!("No revision with prefix {:?} found", prefix);
            }
            (None, Some(targ)) => targ,
            (Some(a), Some(b)) if a != *b => {
                anyhow::bail!("More than one revision matches prefix {:?}", prefix);
            }
            (Some(_), Some(targ)) => targ,
            (Some(targ), None) => targ,
        };
        if let Some(db_rev) = db_rev {
            if !migrate.quiet {
                if Some(&db_rev) == db_migration.as_ref() {
                    eprintln!("Database is up to date. Revision {}", db_rev);
                } else {
                    eprintln!(
                        "Database is up to date. \
                        Revision {} is the ancestor of the latest {}",
                        db_rev,
                        db_migration.as_ref().map(|x| &x[..]).unwrap_or("initial"),
                    );
                }
            }
            return Err(ExitCode::new(0))?;
        }
        Some(target_rev.clone())
    } else {
        None
    };

    if let Some(db_migration) = &db_migration {
        skip_revisions(&mut migrations, db_migration)?;
    };
    if let Some(target_rev) = &target_rev {
        while let Some((key, _)) = migrations.back() {
            if key != target_rev {
                migrations.pop_back();
            } else {
                break;
            }
        }
    }
    if migrations.is_empty() {
        if !migrate.quiet {
            eprintln!(
                "Everything is up to date. Revision {}",
                db_migration.as_ref().map(|x| &x[..]).unwrap_or("initial")
            );
        }
        return Ok(());
    }
    // TODO(tailhook) use special transaction facility
    cli.execute("START TRANSACTION").await?;
    for (_, migration) in migrations {
        let data = fs::read_to_string(&migration.path)
            .await
            .context("error re-reading migration file")?;
        cli.execute(data).await?;
        if !migrate.quiet {
            eprintln!(
                "Applied {} ({})",
                migration.data.id,
                Path::new(migration.path.file_name().unwrap()).display()
            );
        }
    }
    cli.execute("COMMIT").await?;
    return Ok(());
}
