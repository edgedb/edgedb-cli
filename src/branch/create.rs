use crate::branch::context::Context;
use crate::branch::option::Create;
use crate::connect::Connection;
use crate::print;

pub async fn main(
    options: &Create,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    eprintln!("Creating branch '{}'...", options.name);

    create_branch(connection, &options.name, options.from.as_ref().unwrap_or(&context.branch), options.empty, options.copy_data).await?;
    Ok(())
}


pub async fn create_branch(connection: &mut Connection, name: &String, from: &String, empty: bool, copy_data: bool) -> anyhow::Result<()> {
    let branch_name = edgeql_parser::helpers::quote_name(name);
    let query: String;

    if empty {
        query = format!("create empty branch {}", branch_name)
    } else {
        let branch_type = if copy_data { "data" } else { "schema" };

        query = format!(
            "create {} branch {} from {}",
            branch_type,
            branch_name,
            edgeql_parser::helpers::quote_name(&from)
        )
    }

    let status = connection.execute(&query, &()).await?;

    print::completion(status);

    Ok(())
}