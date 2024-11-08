use termimad::crossterm::style::Stylize;

use crate::branch::context::Context;
use crate::branch::option::Current;
use crate::connect::Connection;

pub async fn main(
    options: &Current,
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
