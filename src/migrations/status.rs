use colorful::Colorful;
use indexmap::IndexMap;

use crate::async_try;
use crate::commands::{Options, ExitCode};
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::create::{execute_start_migration, CurrentMigration};
use crate::migrations::create::{execute_if_connected};
use crate::migrations::migration::{self, MigrationFile};
use crate::migrations::options::ShowStatus;
use crate::print;


async fn ensure_diff_is_empty(cli: &mut Connection, ctx: &Context)
    -> Result<(), anyhow::Error>
{
    let data = cli.query_required_single::<CurrentMigration, _>(
        "DESCRIBE CURRENT MIGRATION AS JSON",
        &(),
    ).await?;
    if !data.confirmed.is_empty() || !data.complete {
        if !ctx.quiet {
            eprintln!("Detected differences between \
                the database schema and the schema source, \
                in particular:");
            let changes = data.confirmed.iter()
                .chain(data.proposed.iter()
                    .flat_map(|p| p.statements.iter().map(|s| &s.text)));
            for text in changes.take(3) {
                eprintln!("    {}",
                    text.lines().collect::<Vec<_>>()
                    .join("\n    "));
            }
            let changes = data.confirmed.len() +
                data.proposed.map(|_| 1).unwrap_or(0);
            if changes > 3 {
                eprintln!("... and {} more changes", changes - 3);
            }
            print::error("Some migrations are missing.");
            eprintln!("  Use `edgedb migration create`.");
        }
        return Err(ExitCode::new(2).into());
    }
    Ok(())
}

pub async fn status(cli: &mut Connection, _options: &Options,
    status: &ShowStatus)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_project_or_config(&status.cfg, status.quiet).await?;
    let migrations = migration::read_all(&ctx, true).await?;
    match up_to_date_check(cli, &ctx, &migrations).await? {
        Some(_) if status.quiet => Ok(()),
        Some(migration) => {
            if print::use_color() {
                eprintln!(
                    "{} Last migration: {}.",
                    "Database is up to date.".bold().light_green(),
                    migration.bold().white(),
                );
            } else {
                eprintln!(
                    "Database is up to date. Last migration: {}.",
                    migration,
                );
            }
            Ok(())
        }
        None => Err(ExitCode::new(3).into()),
    }
}

pub async fn migrations_applied(cli: &mut Connection, ctx: &Context,
                                migrations: &IndexMap<String, MigrationFile>)
    -> Result<Option<String>, anyhow::Error>
{
    let db_migration: Option<String> = cli.query_single(r###"
            WITH Last := (SELECT schema::Migration
                          FILTER NOT EXISTS .<parents[IS schema::Migration])
            SELECT name := assert_single(Last.name)
        "###, &()).await?;
    if db_migration.as_ref() != migrations.keys().last() {
        if !ctx.quiet {
            if let Some(db_migration) = &db_migration {
                if let Some(_) = migrations.get(db_migration) {
                    let mut iter = migrations.keys()
                        .skip_while(|k| k != &db_migration);
                    iter.next(); // skip db_migration itself
                    let first = iter.next().unwrap();  // we know it's not last
                    let count = iter.count() + 1;
                    print::error(format!(
                        "Database is at migration {db:?} while sources \
                        contain {n} migrations ahead, \
                        starting from {first:?}({first_file})",
                        db=db_migration,
                        n=count,
                        first=first,
                        first_file=migrations[first].path.display()
                    ));
                } else {
                    print::error(format!(
                        "There is no database revision {} in the filesystem.",
                        db_migration,
                    ));
                    eprintln!("  Consider updating sources.");
                }
            } else {
                print::error(format!(
                    "Database is empty. While there are {} migrations \
                    on the filesystem.",
                    migrations.len(),
                ));
                eprintln!("  Run `edgedb migrate` to apply.");
            }
        }
        return Ok(None);
    }
    Ok(Some(db_migration.unwrap_or_else(|| String::from("initial"))))
}

pub async fn up_to_date_check(cli: &mut Connection, ctx: &Context,
                              migrations: &IndexMap<String, MigrationFile>)
    -> Result<Option<String>, anyhow::Error>
{
    let result = migrations_applied(cli, ctx, migrations).await?;
    if result.is_none() {
        // No sense checking database difference
        return Ok(None);
    }
    execute_start_migration(ctx, cli).await?;
    async_try! {
        async {
            ensure_diff_is_empty(cli, ctx).await
        },
        finally async {
            execute_if_connected(cli, "ABORT MIGRATION").await
        }
    }?;
    Ok(result)
}
