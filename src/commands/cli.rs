use async_std::task;

use crate::cli;
use crate::cli::directory_check;
use crate::cloud::main::cloud_main;
use crate::options::{Options, Command};
use crate::commands::parser::{Common, MigrationCmd, Migration};
use crate::commands;
use crate::migrations;
use crate::portable;
use crate::print::style::Styler;
use crate::non_interactive;


pub fn main(options: Options) -> Result<(), anyhow::Error> {
    match options.subcommand.as_ref().expect("subcommand is present") {
        Command::Common(cmd) => {
            let cmdopt = commands::Options {
                command_line: true,
                styler: if atty::is(atty::Stream::Stdout) {
                    Some(Styler::dark_256())
                } else {
                    None
                },
                conn_params: options.create_connector()?,
            };
            directory_check::check_and_warn();
            match cmd {
                // Process commands that don't need connection first
                Common::Migration(
                    Migration { subcommand: MigrationCmd::Log(mlog), .. }
                ) if mlog.from_fs => {
                    task::block_on(migrations::log_fs(&cmdopt, &mlog)).into()
                }
                Common::Migration(
                    Migration { subcommand: MigrationCmd::Edit(params), .. }
                ) if params.no_check => {
                    task::block_on(
                        migrations::edit_no_check(&cmdopt, &params)
                    ).into()
                }
                // Otherwise connect
                cmd => {
                    task::block_on(async {
                        let mut conn = cmdopt.conn_params.connect().await?;
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
            portable::server_main(cmd)
        }
        Command::Instance(cmd) => {
            directory_check::check_and_error()?;
            portable::instance_main(cmd, &options)
        }
        Command::Project(cmd) => {
            directory_check::check_and_error()?;
            portable::project_main(cmd, &options)
        }
        Command::Query(q) => {
            directory_check::check_and_warn();
            task::block_on(non_interactive::main(&q, &options)).into()
        }
        Command::_SelfInstall(s) => {
            cli::install::main(s)
        }
        Command::_GenCompletions(s) => {
            cli::install::gen_completions(s)
        }
        Command::CliCommand(c) => {
            cli::main(c)
        },
        Command::Info(info) => {
            task::block_on(commands::info(&options, info)).into()
        }
        Command::UI(c) => {
            commands::show_ui(&options, c)
        }
        Command::Cloud(c) => {
            cloud_main(c, &options.cloud_options)
        }
    }
}
