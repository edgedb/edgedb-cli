use async_std::task;

use crate::cli::directory_check;
use crate::cli::self_install;
use crate::cli::self_migrate;
use crate::cli::self_upgrade;
use crate::options::{Options, Command};
use crate::cli::options::SelfSubcommand;
use crate::commands::parser::{Common, MigrationCmd, Migration};
use crate::non_interactive;
use crate::commands;
use crate::migrations;
use crate::server;
use crate::project;
use crate::print::style::Styler;


pub fn main(options: Options) -> Result<(), anyhow::Error> {
    let cmdopt = commands::Options {
        command_line: true,
        styler: if atty::is(atty::Stream::Stdout) {
            Some(Styler::dark_256())
        } else {
            None
        },
        conn_params: options.conn_params.clone(),
    };
    match options.subcommand.as_ref().expect("subcommand is present") {
        Command::Common(cmd) => {
            directory_check::check_and_warn();
            match cmd {
                Common::Migration(
                    Migration { subcommand: MigrationCmd::Log(mlog), .. }
                ) if mlog.from_fs => {
                    // no need for connection
                    task::block_on(
                        migrations::log_fs(&cmdopt, &mlog)).into()
                }
                cmd => {
                    task::block_on(async {
                        let mut conn = options.conn_params.connect().await?;
                        commands::execute::common(
                            &mut conn, cmd, &cmdopt
                        ).await?;
                        Ok(())
                    }).into()
                }
            }
        },
        Command::Server(cmd) => {
            directory_check::check_and_error()?;
            server::main(cmd)
        }
        Command::Instance(cmd) => {
            directory_check::check_and_error()?;
            server::instance_main(cmd)
        }
        Command::Project(cmd) => {
            directory_check::check_and_error()?;
            project::main(cmd)
        }
        Command::Query(q) => {
            directory_check::check_and_warn();
            task::block_on(async {
                let mut conn = options.conn_params.connect().await?;
                for query in &q.queries {
                    non_interactive::query(&mut conn, query, &options).await?;
                }
                Ok(())
            }).into()
        },
        Command::_SelfInstall(s) => {
            self_install::main(s)
        }
        Command::_GenCompletions(s) => {
            self_install::gen_completions(s)
        }
        Command::SelfCommand(c) => match &c.subcommand {
            SelfSubcommand::Upgrade(s) => self_upgrade::main(s),
            SelfSubcommand::Install(s) => self_install::main(s),
            SelfSubcommand::Migrate(s) => self_migrate::main(s),
        },
        Command::Authenticate(cmd) => {
            task::block_on(async {
                server::authenticate::authenticate(cmd, &options).await?;
                Ok(())
            }).into()
        },
    }
}
