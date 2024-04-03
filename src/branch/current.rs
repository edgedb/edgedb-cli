use crossterm::style::Stylize;
use crate::branch::context::Context;
use crate::branch::option::Current;
use crate::connect::Connection;

pub async fn main(
    options: &Current,
    context: &Context,
) -> anyhow::Result<()> {
    if options.shell {
        if let Some(branch) = &context.branch {
            eprintln!("{}", branch);
        }

        return Ok(())
    }

    match &context.branch {
        Some(branch) => eprintln!("The current branch is '{}'", branch.clone().green()),
        None => anyhow::bail!("No project found")
    }

    Ok(())
}
