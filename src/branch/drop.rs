use crate::branch::context::Context;
use crate::branch::option::Drop;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::portable::exit_codes;
use crate::{print, question};

pub async fn main(
    options: &Drop,
    context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    if (context.auto_config.current_branch == options.branch) {
        anyhow::bail!(
            "Dropping the currently active branch is not supported, please switch to a \
            different branch to drop this one with `edgedb branch switch <branch>`"
        );
    }

    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to drop the branch {:?}?",
            options.branch
        ));
        if !connection.ping_while(q.async_ask()).await? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }

    let status = connection
        .execute(&format!("drop branch {}", options.branch), &())
        .await?;

    print::completion(status);

    Ok(())
}
