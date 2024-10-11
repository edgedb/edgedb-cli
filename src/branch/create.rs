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

    let from = if let Some(from) = &options.from {
        from.clone()
    } else {
        context.get_current_branch(connection).await?
    };

    create_branch(
        connection,
        &options.name,
        &from,
        options.empty,
        options.copy_data,
    )
    .await?;
    Ok(())
}

pub async fn create_branch(
    connection: &mut Connection,
    name: &str,
    from: &str,
    empty: bool,
    copy_data: bool,
) -> anyhow::Result<()> {
    let new_branch = edgeql_parser::helpers::quote_name(name);

    let query = if empty {
        if copy_data {
            eprintln!("WARNING: when --empty is used, --copy-data will be ignored");
        }

        format!("create empty branch {}", new_branch)
    } else {
        let kind = if copy_data { "data" } else { "schema" };

        let from = edgeql_parser::helpers::quote_name(from);
        format!("create {kind} branch {new_branch} from {from}")
    };

    let (status, _warnings) = connection.execute(&query, &()).await?;
    print::completion(status);
    Ok(())
}
