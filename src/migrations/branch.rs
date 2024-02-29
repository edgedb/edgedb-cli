use fs_err as fs;

use std::ops::Index;
use indexmap::IndexMap;
use crate::connect::Connection;
use crate::migrations::{Context, create, db_migration, migrate, migration};
use crate::migrations::create::MigrationKey;
use crate::migrations::db_migration::{DBMigration, read_all};
use crate::migrations::dev_mode::rebase_to_schema;
use crate::migrations::extract::DatabaseMigration;
use crate::migrations::migrate::apply_migrations_inner;
use crate::print;

pub struct RebaseMigrations {
    source_migrations: IndexMap<String, DBMigration>,
    target_migrations: IndexMap<String, DBMigration>,
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

    let last_source_migration_id = source_migrations.last().unwrap().0;

    if !target_migrations.contains_key(last_source_migration_id) {
        anyhow::bail!("Cannot fast-forward branch '{0}' to '{1}': The branch '{1}' contains migrations that are not apart of the branch '{0}'", source.database(), target.database())
    }

    let target_migration_features = target_migrations.split_off(target_migrations.get_full(last_source_migration_id).unwrap().0);

    Ok(RebaseMigrations {
        source_migrations,
        target_migrations: target_migration_features,
    })
}

pub async fn do_rebase(connection: &mut Connection, context: &Context, rebase_migrations: RebaseMigrations) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let temp_ctx = Context {
        schema_dir: temp_dir.path().to_path_buf(),
        edgedb_version: None,
        quiet: false,
    };

    // write all the migrations to disc
    let mut key_index = 0;
    let feature_index_offset = rebase_migrations.source_migrations.len();
    let mut base_migrations_iter = rebase_migrations.source_migrations.into_iter().enumerate();
    let mut feature_migrations_iter = rebase_migrations.target_migrations.into_iter().enumerate();

    loop {
        match (base_migrations_iter.next(), feature_migrations_iter.next()) {
            (base, feature) if base.is_some() || feature.is_some() => {
                if let Some((_, base_migration)) = base {
                    let key = MigrationKey::Index((key_index + 1) as u64);
                    create::write_migration(&temp_ctx, &DatabaseMigration {
                        key, migration: base_migration.1
                    }, false).await?;
                }

                if let Some((_, feature_migration)) = feature {
                    let key = MigrationKey::Index((key_index + feature_index_offset + 1) as u64);
                    create::write_migration(&temp_ctx, &DatabaseMigration {
                        key, migration: feature_migration.1
                    }, false).await?;
                }

                key_index += 1;
            },
            _ => break
        }
    }

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

    migrate::apply_migrations(connection, &migrations.split_off(feature_index_offset), context, true).await?;

    Ok(())
}