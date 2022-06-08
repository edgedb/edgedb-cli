use anyhow::Context as _;
use async_std::fs;
use async_std::path::Path;
use async_std::stream::StreamExt;
use colorful::Colorful;
use edgedb_client::client::Connection;
use linked_hash_map::LinkedHashMap;

use crate::commands::Options;
use crate::commands::ExitCode;
use crate::commands::parser::Migrate;
use crate::migrations::timeout;
use crate::migrations::context::Context;
use crate::migrations::migration::{self, MigrationFile};
use crate::print::{self, echo, Highlight};
use crate::error_display::print_query_error;


fn skip_revisions(migrations: &mut LinkedHashMap<String, MigrationFile>,
    db_migration: &str)
    -> anyhow::Result<()>
{
    while let Some((key, _)) = migrations.pop_front() {
        if key == db_migration {
            return Ok(())
        }
    }
    anyhow::bail!("There is no database revision {} \
        in the filesystem. Consider updating sources.",
        db_migration);
}

async fn check_revision_in_db(cli: &mut Connection, prefix: &str)
    -> Result<Option<String>, anyhow::Error>
{
    let mut items = cli.query::<String, _>(r###"
        SELECT name := schema::Migration.name
        FILTER name LIKE <str>$0
        "###,
        &(format!("{}%", prefix),),
    ).await?;
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
    return Ok(all_similar.pop())
}

pub async fn migrate(cli: &mut Connection, _options: &Options,
    migrate: &Migrate)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_project_or_config(&migrate.cfg)?;

    let mut migrations = migration::read_all(&ctx, true).await?;
    let db_migration: Option<String> = cli.query_row_opt(r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := Last.name
        "###, &()).await?;

    let target_rev = if let Some(prefix) = &migrate.to_revision {
        let db_rev = check_revision_in_db(cli, prefix).await?;
        let file_revs = migrations.keys()
            .filter(|r| r.starts_with(prefix))
            .collect::<Vec<_>>();
        if file_revs.len() > 1 {
            anyhow::bail!("More than one revision matches prefix {:?}",
                prefix);
        }
        let target_rev = match (&db_rev, file_revs.last()) {
            (None, None) => {
                anyhow::bail!("No revision with prefix {:?} found",
                    prefix);
            }
            (None, Some(targ)) => targ,
            (Some(a), Some(b)) if a != *b => {
                anyhow::bail!("More than one revision matches prefix {:?}",
                    prefix);
            }
            (Some(_), Some(targ)) => targ,
            (Some(targ), None) => targ,
        };
        if let Some(db_rev) = db_rev {
            if !migrate.quiet {
                let mut msg = "Database is up to date.".to_string();
                if print::use_color() {
                    msg = format!("{}", msg.bold().light_green());
                }
                if Some(&db_rev) == db_migration.as_ref() {
                    eprintln!("{} Revision {}", msg, db_rev);
                } else {
                    eprintln!("{} Revision {} is the ancestor of the latest {}",
                        msg,
                        db_rev,
                        db_migration.as_ref()
                            .map(|x| &x[..]).unwrap_or("initial"),
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
            if print::use_color() {
                eprintln!(
                    "{} Revision {}",
                    "Everything is up to date.".bold().light_green(),
                    db_migration
                        .as_ref()
                        .map(|x| &x[..])
                        .unwrap_or("initial")
                        .bold()
                        .white(),
                );
            } else {
                eprintln!(
                    "Everything is up to date. Revision {}",
                    db_migration
                        .as_ref()
                        .map(|x| &x[..])
                        .unwrap_or("initial"),
                );
            }
        }
        return Ok(());
    }
    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    let transaction = async {
        cli.execute("START TRANSACTION").await?;
        for (_, migration) in migrations {
            let data = fs::read_to_string(&migration.path).await
                .context("error re-reading migration file")?;
            cli.execute(&data).await.map_err(|err| {
                match print_query_error(&err, &data, false) {
                    Ok(()) => ExitCode::new(1).into(),
                    Err(err) => err,
                }
            })?;
            if !migrate.quiet {
                if print::use_color() {
                    eprintln!(
                        "{} {} ({})",
                        "Applied".bold().light_green(),
                        migration.data.id.bold().white(),
                        Path::new(migration.path.file_name().unwrap()).display(),
                    );
                } else {
                    eprintln!(
                        "Applied {} ({})",
                        migration.data.id,
                        Path::new(migration.path.file_name().unwrap()).display(),
                    );
                }
            }
        }
        cli.execute("COMMIT").await?;
        Ok(())
    }.await;
    if cli.is_consistent() {
        let timeout = timeout::restore_for_transaction(cli, old_timeout).await;
        transaction.and(timeout)?;
    } else {
        transaction?;
    };
    if db_migration.is_none() {
        let ddl_setting = cli.query_row(r#"
            SELECT exists(
                SELECT prop := (
                        SELECT schema::ObjectType
                        FILTER .name = 'cfg::DatabaseConfig'
                    ).properties.name
                FILTER prop = "allow_bare_ddl"
            )
        "#, &()).await?;
        if ddl_setting {
            cli.execute(r#"
                CONFIGURE CURRENT DATABASE SET allow_bare_ddl :=
                    cfg::AllowBareDDL.NeverAllow;
            "#).await?;
            echo!("Note: adding first migration disables DDL. \
                   More info: https://edgedb.com/p/bare_ddl".fade());
        }
    }
    return Ok(())
}
