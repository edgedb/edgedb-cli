use colorful::Colorful;

use uuid::Uuid;
use crate::branch::context::Context;
use crate::branch::option::Rebase;
use crate::connect::Connection;
use crate::migrations::rebase::{do_rebase, get_diverging_migrations, write_rebased_migration_files};
use crate::options::Options;
use crate::{migrations, print};
use crate::branch::connections::get_connection_to_modify;

pub async fn main(options: &Rebase, context: &Context, source_connection: &mut Connection, cli_opts: &Options) -> anyhow::Result<()> {
    if options.target_branch == context.branch {
        anyhow::bail!("Cannot rebase the current branch on top of itself");
    }

    let temp_branch = clone_target_branch(&options.target_branch, source_connection).await?;

    let mut temp_branch_connection = cli_opts.create_connector().await?.database(&temp_branch)?.connect().await?;

    match rebase(&temp_branch, source_connection, &mut temp_branch_connection, context, cli_opts, !options.no_apply).await {
        Err(e) => {
            print::error(e);

            eprintln!("Cleaning up cloned branch...");
            let mut rename_connection = get_connection_to_modify(&temp_branch, cli_opts, source_connection).await?;
            let result = rename_connection.connection.execute(&format!("drop branch {} force", edgeql_parser::helpers::quote_name(&temp_branch)), &()).await?;

            print::completion(result);

            rename_connection.clean().await
        }
        Ok(_) => anyhow::Ok(())
    }
}

async fn rebase(branch: &String, source_connection: &mut Connection, target_connection: &mut Connection, context: &Context, cli_opts: &Options, apply_migrations: bool) -> anyhow::Result<()> {
    let mut migrations = get_diverging_migrations(source_connection, target_connection).await?;

    migrations.print_status();

    let migration_context = migrations::Context::for_project(&context.project_config)?;
    do_rebase(&mut migrations, &migration_context).await?;

    if apply_migrations {
        write_rebased_migration_files(&migrations, &migration_context, target_connection).await?;
    }

    // drop source branch
    eprintln!("\nReplacing '{}' with rebased version...", &context.branch);
    let status = target_connection.execute(&format!("drop branch {} force", edgeql_parser::helpers::quote_name(&context.branch)), &()).await?;
    print::completion(status);
    rename_temp_to_source(branch, &context.branch, cli_opts, target_connection).await?;

    eprintln!("Done!");
    anyhow::Ok(())
}

async fn rename_temp_to_source(temp_branch: &String, source_branch: &String, options: &Options, connection: &mut Connection) -> anyhow::Result<()> {
    let mut rename_connection = get_connection_to_modify(&temp_branch, options, connection).await?;

    let status = rename_connection.connection.execute(&format!(
        "alter branch {} force rename to {}",
        edgeql_parser::helpers::quote_name(&temp_branch),
        edgeql_parser::helpers::quote_name(&source_branch)
    ), &()).await?;

    print::completion(status);

    rename_connection.clean().await?;

    Ok(())
}

async fn clone_target_branch(branch: &String, connection: &mut Connection) -> anyhow::Result<String> {
    eprintln!("Cloning target branch '{}' for rebase...", branch.clone().green());

    let temp_branch_name = Uuid::new_v4().to_string();

    let status = connection.execute(
        &format!(
            "create data branch {} from {}",
            edgeql_parser::helpers::quote_name(&temp_branch_name),
            edgeql_parser::helpers::quote_name(branch)
        ),
        &()
    ).await?;

    print::completion(status);

    Ok(temp_branch_name)
}
