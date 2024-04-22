use crate::branch::connections::connect_if_branch_exists;
use crate::branch::context::Context;
use crate::branch::create::create_branch;
use crate::branch::main::verify_server_can_use_branches;
use crate::branch::option::Switch;
use crate::commands::CommandResult;
use crate::connect::Connector;

pub async fn main(
    options: &Switch,
    context: &Context,
    connector: &mut Connector,
) -> anyhow::Result<Option<CommandResult>> {
    if context.branch.is_none() {
        anyhow::bail!("Cannot switch branches: No project found");
    }

    let current_branch = context.branch.as_ref().unwrap();

    if current_branch == &options.target_branch {
        anyhow::bail!("Already on '{}'", options.target_branch);
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

        if !branches.contains(&options.target_branch) {
            if options.create {
                eprintln!("Creating '{}'...", &options.target_branch);
                create_branch(
                    &mut connection,
                    &options.target_branch,
                    options.from.as_ref().unwrap_or(current_branch),
                    options.empty,
                    options.copy_data,
                )
                .await?;
            } else {
                anyhow::bail!("Branch '{}' doesn't exists", options.target_branch)
            }
        }
    } else {
        // try to connect to the target branch
        let target_branch_connector = connector.branch(&options.target_branch)?;
        match connect_if_branch_exists(target_branch_connector).await? {
            Some(mut connection) => {
                verify_server_can_use_branches(&mut connection).await?;
            }
            None => anyhow::bail!("The target branch doesn't exist."),
        };
    }

    eprintln!(
        "Switching from '{}' to '{}'",
        current_branch, options.target_branch
    );

    context.update_branch(&options.target_branch).await?;

    Ok(Some(CommandResult {
        new_branch: Some(options.target_branch.clone()),
    }))
}
