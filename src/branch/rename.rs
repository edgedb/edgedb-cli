use crate::branch::context::Context;
use crate::branch::option::Rename;
use crate::connect::Connection;
use crate::print;

pub async fn main(
    options: &Rename,
    _context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
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

    eprintln!("Renamed branch {} to {}", options.old_name, options.new_name);
    Ok(())
}
