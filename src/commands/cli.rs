use is_terminal::IsTerminal;

use crate::cli::directory_check;
use crate::cloud::main::cloud_main;
use crate::commands;
use crate::commands::parser::Common;
use crate::migrations;
use crate::migrations::options::{Migration, MigrationCmd as M};
use crate::non_interactive;
use crate::options::{Command, Options};
use crate::portable;
use crate::print::style::Styler;
use crate::watch;
use crate::{branch, cli};

#[tokio::main(flavor = "current_thread")]
async fn common_cmd(
    _options: &Options,
    cmdopt: commands::Options,
    cmd: &Common,
) -> Result<(), anyhow::Error> {
    let mut conn = cmdopt.conn_params.connect().await?;
    commands::execute::common(&mut conn, cmd, &cmdopt).await?;
    Ok(())
}

pub fn main(options: &Options) -> Result<(), anyhow::Error> {
    match options.subcommand.as_ref().expect("subcommand is present") {
        Command::Common(cmd) => {
            let cmdopt = init_command_opts(options)?;
            directory_check::check_and_warn();
            match cmd.as_migration() {
                // Process commands that don't need connection first
                Some(Migration {
                    subcommand: M::Log(mlog),
                    ..
                }) if mlog.from_fs => migrations::log_fs(&cmdopt, mlog),
                Some(Migration {
                    subcommand: M::Edit(params),
                    ..
                }) if params.no_check => migrations::edit_no_check(&cmdopt, params),
                Some(Migration {
                    subcommand: M::UpgradeCheck(params),
                    ..
                }) => migrations::upgrade_check(&cmdopt, params),
                // Otherwise connect
                _ => common_cmd(options, cmdopt, cmd),
            }
        }
        Command::Server(cmd) => {
            directory_check::check_and_error()?;
            portable::server_main(cmd)
        }
        Command::Extension(c) => {
            directory_check::check_and_error()?;
            portable::extension_main(c, options)
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
            non_interactive::noninteractive_main(q, options)
        }
        Command::_SelfInstall(s) => cli::install::main(s),
        Command::_GenCompletions(s) => cli::install::gen_completions(s),
        Command::Cli(c) => cli::main(c),
        Command::Info(info) => commands::info(options, info),
        Command::UI(c) => commands::show_ui(c, options),
        Command::Cloud(c) => cloud_main(c, &options.cloud_options),
        Command::Watch(c) => watch::watch(options, c),
        Command::Branch(c) => {
            let cmdopt = init_command_opts(options)?;
            branch::branch_main(&cmdopt, c)
        }
        Command::HashPassword(cmd) => {
            println!("{}", portable::password_hash(&cmd.password));
            Ok(())
        }
    }
}

fn init_command_opts(options: &Options) -> Result<commands::Options, anyhow::Error> {
    Ok(commands::Options {
        command_line: true,
        styler: if std::io::stdout().is_terminal() {
            Some(Styler::dark_256())
        } else {
            None
        },
        conn_params: options.block_on_create_connector()?,
    })
}
