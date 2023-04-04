use crate::commands::{Options, ExitCode};
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::migration;
use crate::migrations::options::CreateMigration;
use crate::migrations::status::up_to_date_check;
use crate::print::{echo, Highlight};
use crate::question::Confirm;


pub async fn main(cli: &mut Connection, options: &Options,
    create: &CreateMigration)
    -> anyhow::Result<()>
{
    let ctx = Context::from_project_or_config(
        &create.cfg,
        create.non_interactive,
    ).await?;
    let migrations = migration::read_all(&ctx, true).await?;
    let Some(db_rev) = up_to_date_check(cli, &ctx, &migrations).await? else {
        return Err(ExitCode::new(3).into());
    };
    if db_rev == "initial" {
        echo!("No migrations exist. Nothing to do.");
        return Ok(());
    }
    if migrations.len() == 1 {
        echo!("Only single revision exists. Nothing to do.");
        return Ok(());
    }
    if !create.non_interactive {
        confirm_squashing(&db_rev).await?;
    }
    // 3. Create new revision as first revision
    // 4. Ask to create fixup revision
    // 5. Walkthrough fixup revision
    // 6. If 5 failed, do nothing
    // 7. Write a fixup revision
    // 8. Drop all revisions, write 001.edgeql
    // 9. Drop all old fixup revisions
    // 10. Suggest `git add`.
    todo!();
}

async fn confirm_squashing(db_rev: &str) -> anyhow::Result<()> {
    echo!("Current database revision is:", db_rev.emphasize());
    echo!("Squash operation is non-destructive, but might require manual work \
           if done incorrectly.");
    echo!("");
    echo!("Here is a checklist before doing squash:");
    echo!("  1. Ensure that `./dbschema` dir is comitted");
    echo!("  2. Ensure that other users of the database have the revision \
        above or can create database from scratch.\n     \
                To check specific instance, run:");
    echo!("       edgedb -I <name> migration log --from-db --limit 1"
          .command_hint());
    echo!("  3. Merge version control branches that contain changes \
                if possible.");
    echo!("");
    if !Confirm::new("Proceed?").async_ask().await? {
        return Err(ExitCode::new(0))?;
    }
    Ok(())
}

