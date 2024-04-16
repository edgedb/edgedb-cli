use colorful::Colorful;

use crate::branch::connections::get_connection_to_modify;
use crate::branch::context::Context;
use crate::branch::option::Rebase;
use crate::commands::Options;
use crate::connect::Connection;
use crate::migrations::rebase::{
    do_rebase, get_diverging_migrations, write_rebased_migration_files,
};
use crate::{migrations, print};
use uuid::Uuid;

pub async fn main(
    options: &Rebase,
    context: &Context,
    source_connection: &mut Connection,
    cli_opts: &Options,
) -> anyhow::Result<()> {
    if context.project_config.is_none() {
        anyhow::bail!("Rebase must be used within a project");
    }

    let current_branch = context.branch.as_ref().unwrap();

    if &options.target_branch == current_branch {
        anyhow::bail!("Cannot rebase the current branch on top of itself");
    }

    let temp_branch = clone_target_branch(&options.target_branch, source_connection).await?;

    let mut connector = cli_opts.conn_params.clone();
    let mut temp_branch_connection = connector.branch(&temp_branch)?.connect().await?;

    match rebase(
        &temp_branch,
        source_connection,
        &mut temp_branch_connection,
        context,
        cli_opts,
        !options.no_apply,
    )
    .await
    {
        Err(e) => {
            print::error(e);

            eprintln!("Cleaning up cloned branch...");
            let mut rename_connection =
                get_connection_to_modify(&temp_branch, cli_opts, source_connection).await?;
            let result = rename_connection
                .connection
                .execute(
                    &format!(
                        "drop branch {} force",
                        edgeql_parser::helpers::quote_name(&temp_branch)
                    ),
                    &(),
                )
                .await?;

            print::completion(result);

            rename_connection.clean().await
        }
        Ok(_) => anyhow::Ok(()),
    }
}

async fn rebase(
    branch: &str,
    source_connection: &mut Connection,
    target_connection: &mut Connection,
    context: &Context,
    cli_opts: &Options,
    apply_migrations: bool,
) -> anyhow::Result<()> {
    let mut migrations = get_diverging_migrations(source_connection, target_connection).await?;
    let current_branch = context.branch.as_ref().unwrap();
    let project_config = context.project_config.as_ref().unwrap();
    migrations.print_status();

    let migration_context = migrations::Context::for_project(project_config)?;
    do_rebase(&mut migrations, &migration_context).await?;

    if apply_migrations {
        write_rebased_migration_files(&migrations, &migration_context, target_connection).await?;
    }

    // drop source branch
    eprintln!("\nReplacing '{}' with rebased version...", current_branch);
    let status = target_connection
        .execute(
            &format!(
                "drop branch {} force",
                edgeql_parser::helpers::quote_name(current_branch)
            ),
            &(),
        )
        .await?;
    print::completion(status);
    rename_temp_to_source(branch, current_branch, cli_opts, target_connection).await?;

    eprintln!("Done!");
    anyhow::Ok(())
}

async fn rename_temp_to_source(
    temp_branch: &str,
    source_branch: &str,
    options: &Options,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let mut rename_connection = get_connection_to_modify(temp_branch, options, connection).await?;

    let status = rename_connection
        .connection
        .execute(
            &format!(
                "alter branch {} force rename to {}",
                edgeql_parser::helpers::quote_name(temp_branch),
                edgeql_parser::helpers::quote_name(source_branch)
            ),
            &(),
        )
        .await?;

    print::completion(status);

    rename_connection.clean().await?;

    Ok(())
}

async fn clone_target_branch(branch: &str, connection: &mut Connection) -> anyhow::Result<String> {
    eprintln!("Cloning target branch '{}' for rebase...", branch.green());

    let temp_branch_name = Uuid::new_v4().to_string();

    let status = connection
        .execute(
            &format!(
                "create data branch {} from {}",
                edgeql_parser::helpers::quote_name(&temp_branch_name),
                edgeql_parser::helpers::quote_name(branch)
            ),
            &(),
        )
        .await?;

    print::completion(status);

    Ok(temp_branch_name)
}
