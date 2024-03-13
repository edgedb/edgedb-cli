use crate::branch::connections::{connect_if_branch_exists};
use crate::branch::context::Context;
use crate::branch::create::create_branch;
use crate::branch::main::verify_server_can_use_branches;
use crate::branch::option::Switch;
use crate::connect::{Connector};


pub async fn main(
    options: &Switch,
    context: &Context,
    connector: &mut Connector,
) -> anyhow::Result<()> {
    if context.branch == options.branch {
        anyhow::bail!("Already on '{}'", options.branch);
    }

    if let Some(mut connection) = connect_if_branch_exists(connector).await? {
        verify_server_can_use_branches(&mut connection).await?;

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
                create_branch(&mut connection, &options.branch, options.from.as_ref(), options.empty, options.copy_data).await?;
            } else {
                anyhow::bail!("Branch '{}' doesn't exists", options.branch)
            }
        }
    } else {
        // try to connect to the target branch
        let target_branch_connector = connector.database(&options.branch)?;
        match connect_if_branch_exists(&target_branch_connector).await? {
            Some(mut connection) => {
                verify_server_can_use_branches(&mut connection).await?;
            },
            None => anyhow::bail!("The target branch doesn't exist.")
        };
    }

    eprintln!(
        "Switching from '{}' to '{}'",
        context.branch, options.branch
    );

    context.update_branch(&options.branch).await?;

    Ok(())
}