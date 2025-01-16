use crate::branch::context::Context;
use crate::connect::Connection;
use crate::print;

pub async fn run(
    cmd: &Command,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    eprintln!("Creating branch '{}'...", cmd.name);

    let from = if let Some(from) = &cmd.from {
        from.clone()
    } else {
        context.get_current_branch(connection).await?
    };

    create_branch(connection, &cmd.name, &from, cmd.empty, cmd.copy_data).await?;
    Ok(())
}

/// Create a new branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    /// The name of the branch to create.
    pub name: String,

    /// The optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// Create the branch without any schema or data.
    #[arg(short = 'e', long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// Copy data from the 'base' branch.
    #[arg(alias = "cp", long)]
    pub copy_data: bool,
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

        format!("create empty branch {new_branch}")
    } else {
        let kind = if copy_data { "data" } else { "schema" };

        let from = edgeql_parser::helpers::quote_name(from);
        format!("create {kind} branch {new_branch} from {from}")
    };

    let (status, _warnings) = connection.execute(&query, &()).await?;
    print::completion(status);
    Ok(())
}
