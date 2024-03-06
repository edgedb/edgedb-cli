use std::iter::FromIterator;
use fs_err as fs;

use std::ops::Index;
use anyhow::anyhow;
use crossterm::style::Stylize;
use indexmap::IndexMap;
use crate::connect::Connection;
use crate::migrations::{Context, create, db_migration, migrate, migration};
use crate::migrations::create::{MigrationKey, MigrationToText};
use crate::migrations::db_migration::{DBMigration, read_all};
use crate::migrations::dev_mode::rebase_to_schema;
use crate::migrations::extract::DatabaseMigration;
use crate::migrations::grammar::parse_migration;
use crate::migrations::migrate::apply_migrations_inner;
use crate::migrations::migration::MigrationFile;
use crate::print;

struct RebaseMigration<'a> {
    key: MigrationKey,
    migration: &'a DBMigration,
    parent: Option<&'a str>,
}

impl MigrationToText for RebaseMigration<'_> {
    type StatementsIter<'a> = std::iter::Once<&'a String> where Self: 'a;

    fn key(&self) -> &MigrationKey {
        &self.key
    }

    fn parent(&self) -> anyhow::Result<&str> {
        if let Some(parent) = self.parent {
            return Ok(parent)
        }

        if self.migration.parent_names.is_empty() {
            return Ok("initial");
        }

        Ok(&self.migration.parent_names[0])
    }

    fn id(&self) -> anyhow::Result<&str> {
        Ok(&self.migration.name)
    }

    fn statements<'a>(&'a self) -> Self::StatementsIter<'a> {
        std::iter::once(&self.migration.script)
    }
}

pub struct RebaseMigrations {
    source_migrations: IndexMap<String, DBMigration>,
    target_migrations: IndexMap<String, DBMigration>,
}

impl RebaseMigrations {
    fn flatten(&self) -> anyhow::Result<Vec<RebaseMigration>> {
        let mut result = Vec::new();

        let mut key_index = 0;
        let last_source_migration_id = self.source_migrations.last().map(|v| v.0.as_str()).unwrap_or("initial");
        let mut iter = self.source_migrations.iter();

        loop {
            match iter.next() {
                Some((id, migration)) => {
                    eprintln!("Rebasing {} as base", id.clone().blue());

                    result.push(RebaseMigration {
                        key: MigrationKey::Index(key_index + 1),
                        parent: None,
                        migration
                    });
                    key_index += 1;
                },
                None => break
            }
        }

        iter = self.target_migrations.iter();

        match iter.next() {
            Some((id, migration)) => {
                eprintln!("Rebasing {} as feature onto {}", id.clone().green(), last_source_migration_id.blue());

                result.push(RebaseMigration {
                    key: MigrationKey::Index(key_index + 1),
                    parent: Some(last_source_migration_id),
                    migration
                });
            }
            None => return Ok(result)
        }

        loop {
            match iter.next() {
                Some((id, migration)) => {
                    eprintln!("Rebasing {} as feature", id.clone().green());
                    result.push(RebaseMigration {
                        key: MigrationKey::Index(key_index + 1),
                        parent: None,
                        migration
                    });
                    key_index += 1;
                },
                None => break
            }
        }

        Ok(result)
    }
}

pub async fn get_diverging_migrations(source: &mut Connection, target: &mut Connection) -> anyhow::Result<RebaseMigrations> {
    let source_migrations = read_all(source, true, false).await?;
    let mut target_migrations = read_all(target, true, false).await?;

    if source_migrations.is_empty() {
        return Ok(RebaseMigrations {
            target_migrations,
            source_migrations
        });
    }

    let mut iter = target_migrations.iter().rev();
    loop {
        match iter.next() {
            Some((id, _)) => {
                if source_migrations.contains_key(id) {
                    // diverging point
                    let mut diverging_index = target_migrations.get_index_of(id).unwrap();

                    if diverging_index == target_migrations.len() - 1 {
                        // target is up to date with source
                        anyhow::bail!("Branch {} is already up-to-date", target.database())
                    }

                    // add 1 to diverging_index since we want the migrations not apart of source
                    diverging_index += 1;

                    return Ok(RebaseMigrations {
                        source_migrations,
                        target_migrations: target_migrations.split_off(diverging_index)
                    })
                }
            }
            None => break
        }
    }
    Ok(RebaseMigrations {
        source_migrations,
        target_migrations
    })
}

async fn rebase_migration_ids(context: &Context) -> anyhow::Result<()> {
    let migrations = migration::read_all(context, false).await?;

    for (id, migration_file) in migrations {
        let mut migration_text = fs::read_to_string(&migration_file.path)?;
        let expected_id = migration_file.data.expected_id(&migration_text)?;

        if id != expected_id {
            eprintln!("Updating migration {} to {}", id.red(), expected_id.clone().green());
            migration_text = migration_file.data.replace_id(&migration_text, &expected_id);
            fs::write(&migration_file.path, migration_text)?;
        }
    }

    Ok(())
}

pub async fn do_rebase(connection: &mut Connection, context: &Context, rebase_migrations: RebaseMigrations) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let temp_ctx = Context {
        schema_dir: temp_dir.path().to_path_buf(),
        edgedb_version: None,
        quiet: false,
    };

    // write all the migrations to disc.
    let flattened_migrations = rebase_migrations.flatten()?;
    for migration in flattened_migrations {
        create::write_migration(&temp_ctx, &migration, false).await?;
    }

    // update the IDs of the migrations for the rebase, since we're changing the history the IDs can be invalid so we
    // need to update them.
    rebase_migration_ids(&temp_ctx).await?;

    // remove the old migrations
    for old in migration::read_names(context).await? {
        fs::remove_file(old)?;
    }

    // move the new migrations from temp to schema dir
    for from in migration::read_names(&temp_ctx).await? {
        let to = context
            .schema_dir
            .join("migrations")
            .join(from.file_name().expect(""));
        print::success_msg("Writing", to.display());
        fs::copy(from, to)?;
    }

    // apply the new migrations
    let mut migrations = migration::read_all(context, true).await?;

    let split_index = rebase_migrations.source_migrations.len();

    migrate::apply_migrations(connection, &migrations.split_off(split_index), context, true).await?;

    Ok(())
}