use edgeql_parser::helpers::quote_name;

use crate::connect::Connection;
use crate::commands::{ExitCode, Options};
use crate::commands::parser::{CreateDatabase, DropDatabase, WipeDatabase};
use crate::print;
use crate::question;
use crate::portable::exit_codes;


pub async fn create(cli: &mut Connection, options: &CreateDatabase, _: &Options)
    -> Result<(), anyhow::Error>
{
    let status = cli.execute(
        &format!("CREATE DATABASE {}", quote_name(&options.database_name)),
        &(),
    ).await?;
    print::completion(&status);
    Ok(())
}

pub async fn drop(cli: &mut Connection, options: &DropDatabase, _: &Options)
    -> Result<(), anyhow::Error>
{
    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(
            format!("Do you really want to delete database {:?}?",
                    options.database_name)
        );
        if !q.ask()? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }
    let status = cli.execute(
        &format!("DROP DATABASE {}", quote_name(&options.database_name)),
        &(),
    ).await?;
    print::completion(&status);
    Ok(())
}

pub async fn wipe(cli: &mut Connection, options: &WipeDatabase, _: &Options)
    -> Result<(), anyhow::Error>
{
    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(
            format!("Do you really want to wipe \
                    the contents of the database {:?}?",
                    cli.database())
        );
        if !q.ask()? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }
    let status = cli.execute("RESET SCHEMA TO initial", &()).await?;
    print::completion(&status);
    Ok(())
}
