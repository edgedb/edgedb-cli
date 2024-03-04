use uuid::Uuid;
use crate::branch::context::Context;
use crate::branch::option::Rebase;
use crate::connect::Connection;
use crate::migrations::branch::{do_rebase, get_diverging_migrations};
use crate::options::Options;
use crate::{migrations, print};

pub async fn main(options: &Rebase, context: &Context, connection: &mut Connection, cli_opts: &Options) -> anyhow::Result<()> {
    let temp_branch = clone_target_branch(&options.target_branch, connection).await?;

    let mut temp_branch_connection = cli_opts.create_connector().await?.database(&temp_branch)?.connect().await?;

    let migrations = get_diverging_migrations(&mut temp_branch_connection, connection).await?;

    let migration_context = migrations::Context::for_project(&context.project_config)?;
    do_rebase(&mut temp_branch_connection, &migration_context, migrations).await?;

    // drop old feature branch
    let status = temp_branch_connection.execute(&format!("drop branch {} force", edgeql_parser::helpers::quote_name(&context.auto_config.current_branch)), &()).await?;

    print::completion(status);

    // rename temp branch to feature
    eprintln!("Renaming {} to {}", &temp_branch, &context.auto_config.current_branch);
    rename_temp_to_feature(&temp_branch, &context.auto_config.current_branch, connection).await?;

    Ok(())
}

async fn rename_temp_to_feature(temp_branch: &String, feature_branch: &String, connection: &mut Connection) -> anyhow::Result<()> {
    let status = connection.execute(&format!(
        "alter branch {} force rename to {}",
        edgeql_parser::helpers::quote_name(&temp_branch),
        edgeql_parser::helpers::quote_name(&feature_branch)
    ), &()).await?;

    print::completion(status);

    Ok(())
}

async fn clone_target_branch(branch: &String, connection: &mut Connection) -> anyhow::Result<String> {
    let temp_branch_name = Uuid::new_v4().to_string();

    let status = connection.execute(&format!("create data branch {} from {}", edgeql_parser::helpers::quote_name(&temp_branch_name), edgeql_parser::helpers::quote_name(branch)), &()).await?;

    print::completion(status);

    Ok(temp_branch_name)
}