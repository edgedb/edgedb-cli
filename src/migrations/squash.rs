use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use tokio::fs;

use crate::async_try;
use crate::bug;
use crate::commands::{ExitCode, Options};
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::create::{execute_start_migration, write_migration};
use crate::migrations::create::{first_migration, normal_migration};
use crate::migrations::create::{CurrentMigration, MigrationToText};
use crate::migrations::create::{FutureMigration, MigrationKey};
use crate::migrations::edb::{execute, execute_if_connected};
use crate::migrations::migration;
use crate::migrations::options::CreateMigration;
use crate::migrations::status::migrations_applied;
use crate::migrations::timeout;
use crate::print::{echo, Highlight};
use crate::question::Confirm;

struct TwoStageRemove<'a> {
    ctx: &'a Context,
    filenames: Vec<PathBuf>,
}

pub async fn main(
    cli: &mut Connection,
    _options: &Options,
    create: &CreateMigration,
) -> anyhow::Result<()> {
    let ctx = Context::from_project_or_config(&create.cfg, create.non_interactive).await?;
    let migrations = migration::read_all(&ctx, true).await?;
    let Some(db_rev) = migrations_applied(cli, &ctx, &migrations).await? else {
        return Err(ExitCode::new(3).into());
    };
    let needs_fixup = needs_fixup(cli, &ctx).await?;

    if db_rev == "initial" {
        echo!("No migrations exist. No actions will be taken.");
        return Ok(());
    }
    if migrations.len() == 1 && !needs_fixup {
        echo!("Only a single revision exists. No actions will be taken.");
        return Ok(());
    }
    if !create.non_interactive {
        cli.ping_while(confirm_squashing(&db_rev)).await?;
    }

    let squashed = create_revision(cli, &ctx, create).await?;

    let key = MigrationKey::Fixup {
        target_revision: squashed.id()?.to_owned(),
    };
    let fixup = if needs_fixup {
        if create.non_interactive || cli.ping_while(want_fixup()).await? {
            let parent = Some(&db_rev[..]);
            Some(normal_migration(cli, &ctx, key, parent, create).await?)
        } else {
            None
        }
    } else {
        Some(FutureMigration::empty(key, &db_rev))
    };
    let mut drop = TwoStageRemove::new(&ctx);
    drop.rename_fixups([squashed.id()?, &db_rev[..]]).await?;
    drop.rename_revisions().await?;
    if let Some(fixup) = &fixup {
        write_migration(&ctx, fixup, false).await?;
    }
    write_migration(&ctx, &squashed, false).await?;
    drop.commit().await?;

    print_final_message(fixup.is_some())?;
    Ok(())
}

async fn needs_fixup(cli: &mut Connection, ctx: &Context) -> anyhow::Result<bool> {
    execute_start_migration(ctx, cli).await?;
    async_try! {
        async {
            let data = cli.query_required_single::<CurrentMigration, _>(
                "DESCRIBE CURRENT MIGRATION AS JSON",
                &(),
            ).await?;
            Ok(!data.confirmed.is_empty() || !data.complete)
        },
        finally async {
            execute_if_connected(cli, "ABORT MIGRATION").await
        }
    }
}

async fn create_revision(
    cli: &mut Connection,
    ctx: &Context,
    create: &CreateMigration,
) -> anyhow::Result<FutureMigration> {
    // TODO(tailhook) reset schema to initial
    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    async_try! {
        async {
            execute(cli, "START MIGRATION REWRITE").await?;
            async_try! {
                async {
                    first_migration(cli, ctx, create).await
                },
                finally async {
                    execute_if_connected(cli, "ABORT MIGRATION REWRITE").await
                }
            }
        },
        finally async {
            timeout::restore_for_transaction(cli, old_timeout).await
        }
    }
}

async fn confirm_squashing(db_rev: &str) -> anyhow::Result<()> {
    echo!("Current database revision:", db_rev.emphasize());
    echo!(
        "While squashing migrations is non-destructive, it may lead to manual work \
           if done incorrectly."
    );
    echo!("");
    echo!("Items to check before using --squash:");
    echo!("  1. Ensure that the `./dbschema` dir is committed to version control");
    echo!(
        "  2. Ensure that other users of the database either have all .edgeql files\n     \
                up to the revision above or can create the database from scratch.\n \
                Hint: To see the current revision for a specific instance, run:"
    );
    echo!(
        "       edgedb -I <name> migration log --from-db --newest-first --limit 1".command_hint()
    );
    echo!(
        "  3. Merge version control branches that contain schema changes \
                if possible."
    );
    echo!("");
    if !Confirm::new("Proceed?").async_ask().await? {
        return Err(ExitCode::new(0))?;
    }
    Ok(())
}

async fn want_fixup() -> anyhow::Result<bool> {
    echo!(
        "Your schema differs from the last revision. \
           A fixup file can be created to automate \
           upgrading other instances to a squashed revision. \
           This starts the usual migration creation process."
    );
    echo!("");
    echo!(
        "Feel free to skip this step if you don't have \
           other instances to migrate"
    );
    echo!("");
    Confirm::new("Create a fixup file?").async_ask().await
}

fn print_final_message(fixup_created: bool) -> anyhow::Result<()> {
    if fixup_created {
        echo!("Squash is complete.");
        echo!("");
        echo!(
            "Remember to commit the `dbschema` directory including deleted \
               files and `fixups` subdirectory. Recommended command:"
        );
        echo!("    git add dbschema".command_hint());
        echo!("");
        echo!("The normal migration process will update your migration history:");
        echo!("    edgedb migrate".command_hint());
    } else {
        echo!("Squash is complete.");
        echo!("");
        echo!(
            "Remember to commit the `dbschema` directory including deleted \
               files. Recommended command:"
        );
        echo!("    git add dbschema".command_hint());
        echo!("");
        echo!("You can now wipe your instances and apply the new schema:");
        echo!("    edgedb database wipe".command_hint());
        echo!("    edgedb migrate".command_hint());
    }
    Ok(())
}

impl TwoStageRemove<'_> {
    fn new(ctx: &Context) -> TwoStageRemove<'_> {
        TwoStageRemove {
            ctx,
            filenames: Vec::new(),
        }
    }
    async fn rename_fixups(&mut self, revs: impl IntoIterator<Item = &str>) -> anyhow::Result<()> {
        let dir_path = &self.ctx.schema_dir.join("fixups");
        let mut dir = match fs::read_dir(&dir_path).await {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(e).context(format!("cannot open {:?}", dir_path))?;
            }
        };

        let mut by_target = BTreeMap::new();

        while let Some(item) = dir.next_entry().await? {
            let fname = item.file_name();
            let lossy_name = fname.to_string_lossy();
            if lossy_name.starts_with('.') || !item.file_type().await?.is_file() {
                continue;
            }
            if let Some(stem) = lossy_name.strip_suffix(".edgeql") {
                let mut pair = stem.split('-');
                if let Some((from, to)) = pair.next().zip(pair.next()) {
                    by_target
                        .entry(to.to_owned())
                        .or_insert_with(Vec::new)
                        .push((from.to_owned(), item.path()));
                }
            } else if lossy_name.ends_with(".edgeql.old") {
                self.filenames.push(item.path());
            }
        }

        // Now find fixups unreachable from revs
        let mut queue: Vec<_> = revs.into_iter().map(|r| r.to_owned()).collect();
        while let Some(el) = queue.pop() {
            if let Some(pairs) = by_target.remove(&el) {
                queue.extend(pairs.into_iter().map(|(from, _)| from));
            }
        }

        for pairs in by_target.values() {
            for (_to, path) in pairs {
                self.rename(path).await?;
            }
        }

        Ok(())
    }
    async fn rename_revisions(&mut self) -> anyhow::Result<()> {
        let dir_path = &self.ctx.schema_dir.join("migrations");
        let mut dir = match fs::read_dir(&dir_path).await {
            Ok(dir) => dir,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(e).context(format!("cannot open {:?}", dir_path))?;
            }
        };
        while let Some(item) = dir.next_entry().await? {
            let fname = item.file_name();
            let lossy_name = fname.to_string_lossy();
            if lossy_name.starts_with('.') || !item.file_type().await?.is_file() {
                continue;
            }
            if lossy_name.ends_with(".edgeql") {
                self.rename(&item.path()).await?;
            } else if lossy_name.ends_with(".edgeql.old") {
                self.filenames.push(item.path());
            }
        }
        Ok(())
    }
    async fn commit(self) -> anyhow::Result<()> {
        for fname in self.filenames {
            fs::remove_file(fname).await?;
        }
        Ok(())
    }
    async fn rename(&mut self, path: &Path) -> anyhow::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| bug::error("path without a parent"))?;

        let mut tmp_fname = path
            .file_name()
            .ok_or_else(|| bug::error("path without a filename"))?
            .to_owned();
        tmp_fname.push(".old");
        let tmp_path = dir.join(tmp_fname);

        fs::rename(path, &tmp_path).await?;
        self.filenames.push(tmp_path);
        Ok(())
    }
}
