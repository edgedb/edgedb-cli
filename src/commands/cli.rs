use is_terminal::IsTerminal;

use crate::cli::directory_check;
use crate::{branch, cli};
use crate::cloud::main::cloud_main;
use crate::commands::parser::Common;
use crate::commands;
use crate::migrations::options::{MigrationCmd as M, Migration};
use crate::migrations;
use crate::non_interactive;
use crate::options::{Options, Command};
use crate::portable;
use crate::print::style::Styler;
use crate::watch;

#[tokio::main]
async fn common_cmd(_options: &Options, cmdopt: commands::Options, cmd: &Common)
    -> Result<(), anyhow::Error>
{
    let mut conn = cmdopt.conn_params.connect().await?;
    commands::execute::common(
        &mut conn, cmd, &cmdopt, _options
    ).await?;
    Ok(())
}

pub fn main(options: &Options) -> Result<(), anyhow::Error> {
    match options.subcommand.as_ref().expect("subcommand is present") {
        Command::Common(cmd) => {
            let cmdopt = commands::Options {
                command_line: true,
                styler: if std::io::stdout().is_terminal() {
                    Some(Styler::dark_256())
                } else {
                    None
                },
                conn_params: options.block_on_create_connector()?,
            };
            directory_check::check_and_warn();
            match cmd {
                // Process commands that don't need connection first
                Common::Migration(
                    Migration { subcommand: M::Log(mlog), .. }
                ) if mlog.from_fs => {
                    migrations::log_fs(&cmdopt, &mlog).into()
                }
                Common::Migration(
                    Migration { subcommand: M::Edit(params), .. }
                ) if params.no_check => {
                    migrations::edit_no_check(&cmdopt, &params).into()
                }
                Common::Migration(
                    Migration { subcommand: M::UpgradeCheck(params), .. }
                ) => {
                    migrations::upgrade_check(&cmdopt, params)
                }
                // Otherwise connect
                cmd => common_cmd(options, cmdopt, cmd),
            }
        },
        Command::Server(cmd) => {
            directory_check::check_and_error()?;
            portable::server_main(cmd)
        }
        Command::Instance(cmd) => {
            directory_check::check_and_error()?;
            portable::instance_main(cmd, options)
        }
        Command::Project(cmd) => {
            directory_check::check_and_error()?;
            portable::project_main(cmd, options)
        }
        Command::Query(q) => {
            directory_check::check_and_warn();
            non_interactive::noninteractive_main(&q, options).into()
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
            commands::info(options, info).into()
        }
        Command::UI(c) => {
            commands::show_ui(c, options)
        }
        Command::Cloud(c) => {
            cloud_main(c, &options.cloud_options)
        }
        Command::Watch(c) => {
            watch::watch(options, c)
        },
        Command::Branch(c) => {
            branch::branch_main(options, c)
        }
    }
}
