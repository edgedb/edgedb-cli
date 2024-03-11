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
    let mut modify_connection = get_connection_to_modify(&context.branch, &cli_opts, connection).await?;

    let status = modify_connection.connection
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

    eprintln!("Renamed branch {} to {}", options.old_name, options.new_name);

    if options.old_name == context.branch {
        context.update_branch(&options.new_name).await?;
    }

    modify_connection.clean().await?;

    Ok(())
}
