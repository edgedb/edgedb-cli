use crate::branch::context::Context;
use crate::branch::option::Switch;
use crate::connect::Connection;


pub async fn main(
    options: &Switch,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    // verify the branch exists
    let branches: Vec<String> = connection
        .query(
            "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
            &(),
        )
        .await?;

    if !branches.contains(&options.branch) {
        anyhow::bail!("Branch '{}' doesn't exists", options.branch)
    }

    println!(
        "Switching from '{}' to '{}'",
        context.auto_config.current_branch, options.branch
    );

    if !context.update_branch(&options.branch)? {
        anyhow::bail!("Failed to update branch in edgedb.auto.toml")
    }

    println!("Now on '{}'", options.branch);

    Ok(())
}
