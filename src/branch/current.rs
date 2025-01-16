use termimad::crossterm::style::Stylize;

use crate::branch::context::Context;
use crate::connect::Connection;

pub async fn run(
    options: &Command,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let current_branch = context.get_current_branch(connection).await?;

    if options.plain {
        println!("{current_branch}");
    } else {
        eprintln!("The current branch is '{}'", current_branch.green());
    }
    Ok(())
}

/// Prints the current branch.
#[derive(clap::Args, Clone, Debug)]
pub struct Command {
    /// Print as plain text output to stdout. Prints nothing instead of erroring if the current branch
    /// can't be resolved.
    #[arg(long)]
    pub plain: bool,
}
