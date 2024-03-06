use crate::branch::context::Context;
use crate::branch::option::Create;
use crate::connect::Connection;
use crate::print;

pub async fn main(
    options: &Create,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let branches: Vec<String> = connection.query(
        "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
        &(),
    ).await?;

    if branches.contains(&options.branch) {
        anyhow::bail!("Branch '{}' already exists!", options.branch)
    }

    let source_branch = options
        .from
        .as_ref()
        .unwrap_or(&context.branch);

    let query: String;
    let branch_name = edgeql_parser::helpers::quote_name(&options.branch);

    if options.empty {
        query = format!("create empty branch {}", branch_name)
    } else {
        let branch_type = match options {
            _ if options.copy_data => "data",
            _ => "schema",
        };

        query = format!(
            "create {} branch {} from {}",
            branch_type,
            branch_name,
            edgeql_parser::helpers::quote_name(source_branch)
        )
    }

    eprintln!("Creating branch '{}'...", options.branch);

    let status = connection.execute(&query, &()).await?;

    print::completion(&status);

    context.update_branch(&options.branch).await?;

    Ok(())
}
