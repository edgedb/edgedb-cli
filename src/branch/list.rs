use crate::branch::context::Context;
use crate::branch::option::List;
use crate::connect::Connection;
use crossterm::style::Stylize;

pub async fn main(
    _options: &List,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let branches: Vec<String> = connection
        .query(
            "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
            &(),
        )
        .await?;

    for branch in branches {
        if context.branch.as_ref() == Some(&branch) {
            println!("{} - Current", branch.green());
        } else {
            println!("{}", branch);
        }
    }

    Ok(())
}
