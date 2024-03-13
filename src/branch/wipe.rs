use crate::branch::context::Context;
use crate::branch::option::Wipe;
use crate::commands::ExitCode;
use crate::connect::{Connection, Connector};
use crate::portable::exit_codes;
use crate::{print, question};
use crate::branch::connections::connect_if_branch_exists;

pub async fn main(
    options: &Wipe,
    _context: &Context,
    connector: &mut Connector,
) -> anyhow::Result<()> {
    let connection = connect_if_branch_exists(connector.database(&options.branch)?).await?;

    if connection.is_none() {
        anyhow::bail!("Branch '{}' doesn't exist", &options.branch)
    }

    let mut connection = connection.unwrap();

    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to wipe \
                    the contents of the branch {:?}?",
            options.branch
        ));
        if !connection.ping_while(q.async_ask()).await? {
            print::error("Canceled by user.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }

    let status = connection.execute("RESET SCHEMA TO initial", &()).await?;

    print::completion(status);

    Ok(())
}
