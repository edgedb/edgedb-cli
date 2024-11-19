use crate::branch::context::Context;
use crate::branch::option::Drop;
use crate::branding::BRANDING_CLI_CMD;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::portable::exit_codes;
use crate::{print, question};

pub async fn main(
    options: &Drop,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    let current_branch = context.get_current_branch(connection).await?;

    if current_branch == options.target_branch {
        anyhow::bail!(
            "Dropping the currently active branch is not supported, please switch to a \
            different branch to drop this one with `{BRANDING_CLI_CMD} branch switch <branch>`"
        );
    }

    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to drop the branch {:?}?",
            options.target_branch
        ));
        if !connection.ping_while(q.async_ask()).await? {
            print::error!("Canceled by user.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }

    let mut statement = format!(
        "drop branch {}",
        edgeql_parser::helpers::quote_name(&options.target_branch)
    );

    if options.force {
        statement = format!("{} force", &statement);
    }

    let (status, _warnings) = connection.execute(&statement, &()).await?;

    print::completion(status);

    Ok(())
}
