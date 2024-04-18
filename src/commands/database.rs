use edgeql_parser::helpers::quote_name;

use crate::commands::parser::{CreateDatabase, DropDatabase, WipeDatabase};
use crate::commands::{ExitCode, Options};
use crate::connect::Connection;
use crate::hint::HintExt;
use crate::portable::exit_codes;
use crate::print;
use crate::question;

pub async fn create(
    cli: &mut Connection,
    options: &CreateDatabase,
    _: &Options,
) -> Result<(), anyhow::Error> {
    if cli.get_version().await?.specific().major >= 5 {
        eprintln!("'edgedb database create' is deprecated in EdgeDB 5+. Please use 'edgedb branch create'");
    }

    let status = cli
        .execute(
            &format!("CREATE DATABASE {}", quote_name(&options.database_name)),
            &(),
        )
        .await?;
    print::completion(&status);
    Ok(())
}

pub async fn drop(
    cli: &mut Connection,
    options: &DropDatabase,
    _: &Options,
) -> Result<(), anyhow::Error> {
    if cli.get_version().await?.specific().major >= 5 {
        eprintln!(
            "'edgedb database drop' is deprecated in EdgeDB 5+. Please use 'edgedb branch drop'"
        );
    }

    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to delete database {:?}?",
            options.database_name
        ));
        if !cli.ping_while(q.async_ask()).await? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }
    let status = cli
        .execute(
            &format!("DROP DATABASE {}", quote_name(&options.database_name)),
            &(),
        )
        .await?;
    print::completion(&status);
    Ok(())
}

pub async fn wipe(
    cli: &mut Connection,
    options: &WipeDatabase,
    _: &Options,
) -> Result<(), anyhow::Error> {
    if cli.get_version().await?.specific().major >= 5 {
        eprintln!(
            "'edgedb database wipe' is deprecated in EdgeDB 5+. Please use 'edgedb branch wipe'"
        );
    }

    if cli.get_version().await?.specific() < "3.0-alpha.2".parse().unwrap() {
        return Err(anyhow::anyhow!(
            "The `database wipe` command is only \
                            supported in EdgeDB >= 3.0"
        ))
        .hint("Use `edgedb database drop`, `edgedb database create`")?;
    }
    if !options.non_interactive {
        let q = question::Confirm::new_dangerous(format!(
            "Do you really want to wipe \
                    the contents of the database {:?}?",
            cli.database()
        ));
        if !cli.ping_while(q.async_ask()).await? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
        }
    }
    let status = cli.execute("RESET SCHEMA TO initial", &()).await?;
    print::completion(&status);
    Ok(())
}
