use crate::branch::context::Context;
use crate::branch::option::Wipe;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::portable::exit_codes;
use crate::{print, question};

pub async fn main(
    options: &Wipe,
    _context: &Context,
    connection: &mut Connection,
) -> anyhow::Result<()> {
    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to wipe \
                    the contents of the branch {:?}?",
            options.branch
        ));
        if !connection.ping_while(q.async_ask()).await? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }

    let status = connection.execute("RESET SCHEMA TO initial", &()).await?;

    print::completion(status);

    Ok(())
}
