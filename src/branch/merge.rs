use crate::branch::connections::connect_if_branch_exists;
use crate::branch::context::Context;
use crate::branch::option::{Merge, Rebase};
use crate::connect::Connection;
use crate::migrations;
use crate::migrations::merge::{apply_merge_migration_files, get_merge_migrations, write_merge_migrations};
use crate::options::Options;

pub async fn main(options: &Merge, context: &Context, source_connection: &mut Connection, cli_opts: &Options) -> anyhow::Result<()> {
    if options.target_branch == context.branch {
        anyhow::bail!("Cannot merge the current branch into its self");
    }

    let mut target_connection = match connect_if_branch_exists(
        cli_opts.create_connector().await?.database(&options.target_branch)?
    ).await? {
        Some(connection) => connection,
        None => anyhow::bail!("The branch '{}' doesn't exist", options.target_branch)
    };

    let migration_context = migrations::Context::for_project(&context.project_config)?;
    let mut merge_migrations = get_merge_migrations(source_connection, &mut target_connection).await?;

    eprintln!("Merging {} migration(s) into '{}'...", merge_migrations.target_migrations.len(), source_connection.database());

    write_merge_migrations(&migration_context, &mut merge_migrations).await?;

    if !options.no_apply {
        eprintln!("Applying migrations...");
        apply_merge_migration_files(&merge_migrations, &migration_context, source_connection).await?;
    }

    Ok(())
}