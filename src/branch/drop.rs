use crate::branch::context::Context;
use crate::branch::option::Drop;
use crate::connect::{Connection};
use crate::{print, question};
use crate::commands::ExitCode;
use crate::portable::exit_codes;

pub async fn main(options: &Drop, context: &Context, connection: &mut Connection) -> anyhow::Result<()> {
    // TODO: do we implicitly switch branch here to drop? or do we let the user deal with the
    // 'cannot drop the currently open database branch' error?

    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(
            format!("Do you really want to drop the branch {:?}?",
                    options.branch)
        );
        if !connection.ping_while(q.async_ask()).await? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }

    let status = connection.execute(
        &format!("drop branch {}", options.branch),
        &()
    ).await?;

    print::completion(status);

    if context.auto_config.current_branch == options.branch {
        println!("Switching current branch to 'main'");
        if !context.update_branch(&"main".to_string())? {
            anyhow::bail!("Failed to update edgedb.auto.toml");
        }
    }

    Ok(())
}