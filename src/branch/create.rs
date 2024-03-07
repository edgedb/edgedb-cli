use crate::branch::context::Context;
use crate::branch::option::Create;
use crate::connect::Connection;
use crate::print;

pub async fn main(
    options: &Create,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    eprintln!("Creating branch '{}'...", options.branch);

    create_branch(connection, &options.branch, options.from.as_ref(), options.empty, options.copy_data).await?;
    Ok(())
}


pub async fn create_branch(connection: &mut Connection, name: &String, from: Option<&String>, empty: bool, copy_data: bool) -> anyhow::Result<()> {
    let branch_name = edgeql_parser::helpers::quote_name(name);
    let query: String;

    if empty {
        query = format!("create empty branch {}", branch_name)
    } else if let Some(from_branch) = from {
        let branch_type = if copy_data { "data" } else { "schema" };

        query = format!(
            "create {} branch {} from {}",
            branch_type,
            branch_name,
            edgeql_parser::helpers::quote_name(&from_branch)
        )
    } else {
        anyhow::bail!("Invalid branch configuration");
    }

    let status = connection.execute(&query, &()).await?;

    print::completion(status);

    Ok(())
}