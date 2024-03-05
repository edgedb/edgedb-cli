use colorful::Colorful;
use uuid::Uuid;
use crate::branch::context::Context;
use crate::branch::option::Rebase;
use crate::connect::Connection;
use crate::migrations::branch::{do_rebase, get_diverging_migrations};
use crate::options::Options;
use crate::{async_try, migrations, print};
use crate::branch::connections::get_connection_to_modify;

pub async fn main(options: &Rebase, context: &Context, connection: &mut Connection, cli_opts: &Options) -> anyhow::Result<()> {
    let temp_branch = clone_target_branch(&options.target_branch, connection).await?;

    let mut temp_branch_connection = cli_opts.create_connector().await?.database(&temp_branch)?.connect().await?;

    match rebase(&temp_branch, &mut temp_branch_connection, connection, context, cli_opts).await {
        Err(e) => {
            print::error(e);

            let mut rename_connection = get_connection_to_modify(&temp_branch, cli_opts, connection).await?;
            let result = rename_connection.connection.execute(&format!("drop branch {} force", edgeql_parser::helpers::quote_name(&temp_branch)), &()).await?;

            print::completion(result);

            rename_connection.clean().await
        }
        Ok(_) => anyhow::Ok(())
    }
}

async fn rebase(branch: &String, source_connection: &mut Connection, target_connection: &mut Connection, context: &Context, cli_opts: &Options) -> anyhow::Result<()> {
    let migrations = get_diverging_migrations(source_connection, target_connection).await?;

    let migration_context = migrations::Context::for_project(&context.project_config)?;
    do_rebase(source_connection, &migration_context, migrations).await?;

    // drop old feature branch
    let status = source_connection.execute(&format!("drop branch {} force", edgeql_parser::helpers::quote_name(&context.branch)), &()).await?;

    print::completion(status);

    // rename temp branch to feature
    eprintln!("Recreating branch {}...", &context.branch.clone().light_gray());
    rename_temp_to_feature(branch, &context.branch, cli_opts, source_connection).await?;

    anyhow::Ok(())
}

async fn rename_temp_to_feature(temp_branch: &String, feature_branch: &String, options: &Options, connection: &mut Connection) -> anyhow::Result<()> {
    let mut rename_connection = get_connection_to_modify(&temp_branch, options, connection).await?;

    let status = rename_connection.connection.execute(&format!(
        "alter branch {} force rename to {}",
        edgeql_parser::helpers::quote_name(&temp_branch),
        edgeql_parser::helpers::quote_name(&feature_branch)
    ), &()).await?;

    print::completion(status);

    rename_connection.clean().await?;

    Ok(())
}

async fn clone_target_branch(branch: &String, connection: &mut Connection) -> anyhow::Result<String> {
    let temp_branch_name = Uuid::new_v4().to_string();

    let status = connection.execute(&format!("create data branch {} from {}", edgeql_parser::helpers::quote_name(&temp_branch_name), edgeql_parser::helpers::quote_name(branch)), &()).await?;

    print::completion(status);

    Ok(temp_branch_name)
}