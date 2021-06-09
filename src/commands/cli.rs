use async_std::task;

use crate::options::{Options, Command, SelfSubcommand};
use crate::commands::parser::{Common, MigrationCmd, Migration};
use crate::non_interactive;
use crate::commands;
use crate::migrations;
use crate::self_install;
use crate::self_upgrade;
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
            server::main(cmd)
        }
        Command::Project(cmd) => {
            project::main(cmd)
        }
        Command::CreateSuperuserRole(opt) => {
            task::block_on(async {
                let mut conn = options.conn_params.connect().await?;
                commands::roles::create_superuser(
                    &mut conn, &cmdopt, opt).await?;
                Ok(())
            }).into()
        },
        Command::AlterRole(opt) => {
            task::block_on(async {
                let mut conn = options.conn_params.connect().await?;
                commands::roles::alter(&mut conn, &cmdopt, opt).await?;
                Ok(())
            }).into()
        },
        Command::DropRole(opt) => {
            task::block_on(async {
                let mut conn = options.conn_params.connect().await?;
                commands::roles::drop(&mut conn, &cmdopt, &opt.role).await?;
                Ok(())
            }).into()
        },
        Command::Query(q) => {
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
        },
    }
}
