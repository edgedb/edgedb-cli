use crate::branch::connections::get_connection_to_modify;
use crate::branch::context::Context;
use crate::branch::option::Rename;
use crate::options::Options;
use crate::connect::Connection;
use crate::print;

pub async fn main(
    options: &Rename,
    context: &Context,
    connection: &mut Connection,
    cli_opts: &Options,
) -> anyhow::Result<()> {
    if options.old_name == context.branch {
        let mut modify_connection = get_connection_to_modify(&context.branch, &cli_opts, connection).await?;
        rename(&mut modify_connection.connection, &options).await?;
        modify_connection.clean().await?;
        context.update_branch(&options.new_name).await?;
    } else {
        rename(connection, &options).await?;
    }

    eprintln!("Renamed branch {} to {}", options.old_name, options.new_name);

    Ok(())
}

async fn rename(connection: &mut Connection, options: &Rename) -> anyhow::Result<()> {
    let status = connection
        .execute(
            &format!(
                "alter branch {0}{2} rename to {1}",
                edgeql_parser::helpers::quote_name(&options.old_name),
                edgeql_parser::helpers::quote_name(&options.new_name),
                if options.force { " force" } else { "" }
            ),
            &(),
        )
        .await?;

    print::completion(status);

    Ok(())
}
