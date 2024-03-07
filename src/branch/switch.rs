use crate::branch::context::Context;
use crate::branch::create::create_branch;
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
        if options.create {
            eprintln!("Creating '{}'...", &options.branch);
            create_branch(connection, &options.branch, options.from.as_ref(), options.empty, options.copy_data).await?;
        } else {
            anyhow::bail!("Branch '{}' doesn't exists", options.branch)
        }
    }

    eprintln!(
        "Switching from '{}' to '{}'",
        context.branch, options.branch
    );
    context.update_branch(&options.branch).await?;

    Ok(())
}
