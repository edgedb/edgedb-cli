use async_std::task;

use crate::options::{Options, Command};
use crate::client::{Connection, non_interactive_query};
use crate::commands;
use crate::print::style::Styler;


pub fn main(options: Options) -> Result<(), anyhow::Error> {
    let cmdopt = commands::Options {
        command_line: true,
        styler: if atty::is(atty::Stream::Stdout) {
            Some(Styler::dark_256())
        } else {
            None
        },
    };
    match options.subcommand.as_ref().expect("subcommand is present") {
        Command::Common(cmd) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::execute::common(&mut cli, cmd, &cmdopt).await?;
                Ok(())
            }).into()
        },
        Command::CreateSuperuserRole(opt) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::roles::create_superuser(
                    &mut cli, &cmdopt, opt).await?;
                Ok(())
            }).into()
        },
        Command::AlterRole(opt) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::roles::alter(&mut cli, &cmdopt, opt).await?;
                Ok(())
            }).into()
        },
        Command::DropRole(opt) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::roles::drop(&mut cli, &cmdopt, &opt.role).await?;
                Ok(())
            }).into()
        },
        Command::Query(q) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                for query in &q.queries {
                    non_interactive_query(&mut cli, query, &options).await?;
                }
                Ok(())
            }).into()
        },
    }
}
