use fs_err as fs;

use edgedb_derive::Queryable;

use crate::commands::{ExitCode, Options};
use crate::connect::Connection;
use crate::migrations::create::{MigrationKey, MigrationToText};
use crate::migrations::log::{topology_sort, SortableMigration};
use crate::migrations::options::ExtractMigrations;
use crate::migrations::{create, migration, Context};
use crate::portable::exit_codes;
use crate::{print, question};

#[derive(Queryable, Clone)]
struct Migration {
    name: String,
    script: String,
    parent_names: Vec<String>,
}

impl SortableMigration for Migration {
    type ParentsIter<'a> = std::slice::Iter<'a, String>;

    fn name(&self) -> &str {
        &self.name
    }

    fn is_root(&self) -> bool {
        self.parent_names.is_empty()
    }

    fn iter_parents<'a>(&'a self) -> Self::ParentsIter<'a> {
        self.parent_names.iter()
    }
}

struct DatabaseMigration {
    key: MigrationKey,
    migration: Migration,
}

impl MigrationToText for DatabaseMigration {
    type StatementsIter<'a> = std::iter::Once<&'a String>;

    fn key(&self) -> &MigrationKey {
        &self.key
    }

    fn parent(&self) -> anyhow::Result<&str> {
        let mut iter = self.migration.parent_names.iter();
        match (iter.next(), iter.next()) {
            (None, None) => Ok("initial"),
            (Some(rv), None) => Ok(rv),
            (Some(_), Some(_)) => anyhow::bail!("Cannot yet sync migrations with multiple parents"),
            (None, Some(_)) => unreachable!(),
        }
    }

    fn id(&self) -> anyhow::Result<&str> {
        Ok(&self.migration.name)
    }

    fn statements<'a>(&'a self) -> Self::StatementsIter<'a> {
        std::iter::once(&self.migration.script)
    }
}

pub async fn extract(
    cli: &mut Connection,
    _opts: &Options,
    params: &ExtractMigrations,
) -> anyhow::Result<()> {
    let src_ctx = Context::from_project_or_config(&params.cfg, params.non_interactive).await?;
    let current = migration::read_all(&src_ctx, false).await?;
    let mut disk_iter = current.into_iter();

    let migrations = cli
        .query::<Migration, _>(
            r###"
            SELECT schema::Migration {
                name,
                script,
                parent_names := .parents.name,
            }
            FILTER not exists(.generated_by)
            "###,
            &(),
        )
        .await?;
    let migrations = topology_sort(migrations);
    let mut db_iter = migrations.into_iter().enumerate();
    let temp_dir = tempfile::tempdir()?;
    let temp_ctx = Context {
        schema_dir: temp_dir.path().to_path_buf(),
        edgedb_version: None,
        quiet: false,
    };
    let mut to_delete = Vec::new();

    loop {
        match (disk_iter.next(), db_iter.next()) {
            (existing, Some((i, migration))) => {
                let key = MigrationKey::Index((i + 1) as u64);
                let dm = DatabaseMigration { key, migration };
                if let Some((id, migration_file)) = existing {
                    if dm.id()? != id {
                        if params.non_interactive {
                            if !params.force {
                                anyhow::bail!(
                                    "migration in \"{}\" does not match the \
                                     migration recorded in the database, \
                                     use `--force` to overwrite the file \
                                     with the database version of migration",
                                    migration_file.path.display(),
                                )
                            }
                        } else if !params.force {
                            let q = question::Confirm::new_dangerous(format!(
                                "Migration in \"{}\" does not match the \
                                 migration recorded in the database, \
                                 overwrite with the database version \
                                 of migration?",
                                migration_file.path.display()
                            ));
                            if !q.ask()? {
                                print::error("Canceled.");
                                return Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
                            }
                        }
                        create::write_migration(&temp_ctx, &dm, false).await?;
                    }
                } else {
                    create::write_migration(&temp_ctx, &dm, false).await?;
                }
            }

            (Some((_, migration_file)), None) => {
                if params.non_interactive {
                    if !params.force {
                        anyhow::bail!(
                            "migration in \"{}\" is not present in the \
                             database, use `--force` to automatically remove \
                             the non-matching files",
                            migration_file.path.display()
                        );
                    }
                } else if !params.force {
                    let q = question::Confirm::new_dangerous(format!(
                        "Migration \"{}\" is not present in the database, \
                         remove the non-matching file?",
                        migration_file.path.display()
                    ));
                    if !q.ask()? {
                        print::error("Canceled.");
                        return Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
                    }
                }
                to_delete.push(migration_file.path);
            }

            (None, None) => break,
        }
    }

    let mut updated = false;
    for from in migration::read_names(&temp_ctx).await? {
        let to = src_ctx
            .schema_dir
            .join("migrations")
            .join(from.file_name().expect(""));
        print::success_msg("Writing", to.display());
        fs::copy(from, to)?;
        updated = true;
    }
    for path in to_delete {
        print::success_msg("Removing", path.display());
        fs::remove_file(path)?;
        updated = true;
    }
    if !updated {
        print::success(format!(
            "Migration history in {:?} and in the database are in sync.",
            src_ctx.schema_dir.join("migrations"),
        ));
    }
    Ok(())
}
