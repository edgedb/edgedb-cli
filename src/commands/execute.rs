use crate::connect::Connection;
use const_format::concatcp;
use edgedb_tokio::server_params::{PostgresAddress, PostgresDsn};

use crate::analyze;
use crate::commands::parser::{Common, DatabaseCmd, DescribeCmd, ListCmd};
use crate::commands::{self, branching, CommandResult, Options};
use crate::migrations;
use crate::migrations::options::MigrationCmd;
use crate::print;

pub async fn common(
    cli: &mut Connection,
    cmd: &Common,
    options: &Options,
) -> Result<Option<CommandResult>, anyhow::Error> {
    use Common::*;
    match cmd {
        List(c) => match &c.subcommand {
            ListCmd::Aliases(c) => {
                commands::list_aliases(
                    cli,
                    options,
                    &c.pattern,
                    c.system,
                    c.case_sensitive,
                    c.verbose,
                )
                .await?;
            }
            ListCmd::Casts(c) => {
                commands::list_casts(cli, options, &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Indexes(c) => {
                commands::list_indexes(
                    cli,
                    options,
                    &c.pattern,
                    c.system,
                    c.case_sensitive,
                    c.verbose,
                )
                .await?;
            }
            ListCmd::Databases => {
                commands::list_databases(cli, options).await?;
            }
            ListCmd::Branches => {
                commands::list_branches(cli, options).await?;
            }
            ListCmd::Scalars(c) => {
                commands::list_scalar_types(cli, options, &c.pattern, c.system, c.case_sensitive)
                    .await?;
            }
            ListCmd::Types(c) => {
                commands::list_object_types(cli, options, &c.pattern, c.system, c.case_sensitive)
                    .await?;
            }
            ListCmd::Modules(c) => {
                commands::list_modules(cli, options, &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Roles(c) => {
                commands::list_roles(cli, options, &c.pattern, c.case_sensitive).await?;
            }
        },
        Analyze(c) => {
            analyze::command(cli, c).await?;
        }
        Pgaddr => match cli.get_server_param::<PostgresAddress>() {
            Some(addr) => {
                // < 6.x
                println!("{}", serde_json::to_string_pretty(addr)?);
            }
            None => {
                // >= 6.x
                match cli.get_server_param::<PostgresDsn>() {
                    Some(addr) => {
                        println!("{}", addr.0);
                    }
                    None => print::error(concatcp!(
                        "pgaddr requires ",
                        BRANDING,
                        " to run in DEV mode"
                    )),
                }
            }
        },
        Psql => {
            commands::psql(cli, options).await?;
        }
        Describe(c) => match &c.subcommand {
            DescribeCmd::Object(c) => {
                commands::describe(cli, options, &c.name, c.verbose).await?;
            }
            DescribeCmd::Schema(_) => {
                commands::describe_schema(cli, options).await?;
            }
        },
        Dump(c) => {
            commands::dump(cli, options, c).await?;
        }
        Restore(params) => {
            commands::restore(cli, options, params).await?;
        }
        Configure(c) => {
            commands::configure(cli, options, c).await?;
        }
        Database(c) => match &c.subcommand {
            DatabaseCmd::Create(c) => {
                commands::database::create(cli, c, options).await?;
            }
            DatabaseCmd::Drop(d) => {
                commands::database::drop(cli, d, options).await?;
            }
            DatabaseCmd::Wipe(w) => {
                commands::database::wipe(cli, w, options).await?;
            }
        },
        Branching(branching) => return branching::main(cli, &branching.subcommand, options).await,
        Migrate(params) => {
            migrations::migrate(cli, options, params).await?;
        }
        Migration(m) => match &m.subcommand {
            MigrationCmd::Apply(params) => {
                migrations::migrate(cli, options, params).await?;
            }
            MigrationCmd::Create(params) => {
                migrations::create(cli, options, params).await?;
            }
            MigrationCmd::Status(params) => {
                migrations::status(cli, options, params).await?;
            }
            MigrationCmd::Log(params) => {
                migrations::log(cli, options, params).await?;
            }
            MigrationCmd::Edit(params) => {
                migrations::edit(cli, options, params).await?;
            }
            MigrationCmd::UpgradeCheck(_) => {
                anyhow::bail!("cannot be run in REPL mode");
            }
            MigrationCmd::Extract(params) => {
                migrations::extract(cli, options, params).await?;
            }
            MigrationCmd::UpgradeFormat(params) => {
                migrations::upgrade_format(cli, options, params).await?;
            }
        },
    }
    Ok(None)
}
