use indexmap::IndexMap;
use fs_err as fs;

use crate::connect::Connection;
use crate::migrations::{Context, migrate, migration};
use crate::migrations::create::{MigrationKey, MigrationToText, write_migration};
use crate::migrations::db_migration::{DBMigration, read_all};
use crate::migrations::migration::MigrationFile;
use crate::migrations::rebase::{fix_migration_ids};

pub struct MergeMigrations {
    pub base_migrations: IndexMap<String, MergeMigration>,
    pub target_migrations: IndexMap<String, MergeMigration>
}

impl MergeMigrations {
    fn flatten(&self) -> IndexMap<&String, &MergeMigration> {
        let mut result = IndexMap::new();

        for migration in self.base_migrations.iter() {
            result.insert(migration.0, migration.1);
        }

        for migration in self.target_migrations.iter() {
            result.insert(migration.0, migration.1);
        }

        result
    }
}

pub struct MergeMigration {
    key: MigrationKey,
    id_override: Option<String>,
    migration: DBMigration,
    parent_override: Option<String>
}

impl MigrationToText for MergeMigration {
    type StatementsIter<'a> = std::iter::Once<&'a String> where Self: 'a;

    fn key(&self) -> &MigrationKey {
        &self.key
    }

    fn parent(&self) -> anyhow::Result<&str> {
        Ok(self.parent_override.as_ref().or(self.migration.parent_names.first()).map(|v| v.as_str()).unwrap_or("initial"))
    }

    fn id(&self) -> anyhow::Result<&str> {
        Ok(self.id_override.as_ref().unwrap_or(&self.migration.name))
    }

    fn statements<'a>(&'a self) -> Self::StatementsIter<'a> {
        std::iter::once(&self.migration.script)
    }
}

pub async fn get_merge_migrations(base: &mut Connection, target: &mut Connection) -> anyhow::Result<MergeMigrations> {
    let base_migrations = read_all(base, true, false).await?;
    let mut target_migrations = read_all(target, true, false).await?;

    if base_migrations.len() > target_migrations.len() {
        anyhow::bail!("Source branch contains more migrations than the target branch");
    }

    for (index, (base_migration_id, _)) in base_migrations.iter().enumerate() {
        if let Some((target_migration_id, _)) = target_migrations.get_index(index) {
            if target_migration_id != base_migration_id {
                anyhow::bail!("Migration histories of base and target diverge");
            }
        } else {
            anyhow::bail!("Target branch doesn't contain the migration {} (at index {})", base_migration_id, index)
        }
    }

    let mut target_merge_migrations: IndexMap<String, MergeMigration> = IndexMap::new();
    let mut base_merge_migrations: IndexMap<String, MergeMigration> = IndexMap::new();

    for (index, (id, migration)) in target_migrations.split_off(base_migrations.len()).into_iter().enumerate() {
        target_merge_migrations.insert(id, MergeMigration {
            migration,
            id_override: None,
            key: MigrationKey::Index((base_migrations.len() + index + 1) as u64),
            parent_override: if index == 0 { base_migrations.last().map(|v| v.0.clone()) } else { None }
        });
    }

    for(index, (id, migration)) in target_migrations.into_iter().enumerate() {
        base_merge_migrations.insert(id, MergeMigration {
            migration,
            id_override: None,
            key: MigrationKey::Index((index + 1) as u64),
            parent_override: None
        });
    }

    Ok(MergeMigrations {
        target_migrations: target_merge_migrations,
        base_migrations: base_merge_migrations
    })
}

pub async fn write_merge_migrations(context: &Context, migrations: &mut MergeMigrations) -> anyhow::Result<()> {
    let temp_dir = tempfile::tempdir()?;
    let temp_ctx = Context {
        schema_dir: temp_dir.path().to_path_buf(),
        edgedb_version: None,
        quiet: false,
    };

    for (_, migration) in migrations.flatten() {
        write_migration(&temp_ctx, migration, false).await?;
    }

    fn update_id(old: &String, new: &String, migrations: &mut IndexMap<String, MergeMigration>) {
        if let Some(mut migration) = migrations.remove(old) {
            migration.id_override = Some(new.clone());
            migrations.insert(new.clone(), migration);
        }
    }

    fix_migration_ids(&temp_ctx, |old, new| {
        update_id(old, new, &mut migrations.base_migrations);
        update_id(old, new, &mut migrations.target_migrations);
    }).await?;

    for from in migration::read_names(&temp_ctx).await? {
        let to = context
            .schema_dir
            .join("migrations")
            .join(from.file_name().unwrap());

        fs::copy(from, to)?;
    }

    Ok(())
}

pub async fn apply_merge_migration_files(merge_migrations: &MergeMigrations, context: &Context, connection: &mut Connection) -> anyhow::Result<()> {
    // apply the new migrations
    let migrations: IndexMap<String, MigrationFile> = migration::read_all(context, true).await?.into_iter()
        .filter(|(id, _)| merge_migrations.target_migrations.contains_key(id))
        .collect();

    migrate::apply_migrations(connection, &migrations, context, true).await
}