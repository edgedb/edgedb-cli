use crate::connect::Connection;
use gel_tokio::server_params::{PostgresAddress, PostgresDsn};

use crate::analyze;
use crate::branch;
use crate::branding::BRANDING;
use crate::commands;
use crate::commands::parser::{Common, DatabaseCmd, DescribeCmd, ListCmd};
use crate::commands::Options;
use crate::migrations;
use crate::migrations::options::MigrationCmd;
use crate::print;

pub async fn common(
    conn: &mut Connection,
    cmd: &Common,
    options: &Options,
) -> Result<branch::CommandResult, anyhow::Error> {
    use Common::*;
    match cmd {
        List(c) => match &c.subcommand {
            ListCmd::Aliases(c) => {
                commands::list_aliases(
                    conn,
                    options,
                    &c.pattern,
                    c.system,
                    c.case_sensitive,
                    c.verbose,
                )
                .await?;
            }
            ListCmd::Casts(c) => {
                commands::list_casts(conn, options, &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Indexes(c) => {
                commands::list_indexes(
                    conn,
                    options,
                    &c.pattern,
                    c.system,
                    c.case_sensitive,
                    c.verbose,
                )
                .await?;
            }
            ListCmd::Databases => {
                commands::list_databases(conn, options).await?;
            }
            ListCmd::Branches => {
                commands::list_branches(conn, options).await?;
            }
            ListCmd::Scalars(c) => {
                commands::list_scalar_types(conn, options, &c.pattern, c.system, c.case_sensitive)
                    .await?;
            }
            ListCmd::Types(c) => {
                commands::list_object_types(conn, options, &c.pattern, c.system, c.case_sensitive)
                    .await?;
            }
            ListCmd::Modules(c) => {
                commands::list_modules(conn, options, &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Roles(c) => {
                commands::list_roles(conn, options, &c.pattern, c.case_sensitive).await?;
            }
        },
        Analyze(c) => {
            analyze::command(conn, c).await?;
        }
        Pgaddr => match conn.get_server_param::<PostgresAddress>() {
            Some(addr) => {
                // < 6.x
                println!("{}", serde_json::to_string_pretty(addr)?);
            }
            None => {
                // >= 6.x
                match conn.get_server_param::<PostgresDsn>() {
                    Some(addr) => {
                        println!("{}", addr.0);
                    }
                    None => print::error!("pgaddr requires {BRANDING} to run in DEV mode"),
                }
            }
        },
        Psql => {
            commands::psql(conn, options).await?;
        }
        Describe(c) => match &c.subcommand {
            DescribeCmd::Object(c) => {
                commands::describe(conn, options, &c.name, c.verbose).await?;
            }
            DescribeCmd::Schema(_) => {
                commands::describe_schema(conn, options).await?;
            }
        },
        Dump(c) => {
            commands::dump(conn, options, c).await?;
        }
        Restore(params) => {
            commands::restore(conn, options, params).await?;
        }
        Configure(c) => {
            commands::configure(conn, options, c).await?;
        }
        Database(c) => match &c.subcommand {
            DatabaseCmd::Create(c) => {
                commands::database::create(conn, c, options).await?;
            }
            DatabaseCmd::Drop(d) => {
                commands::database::drop(conn, d, options).await?;
            }
            DatabaseCmd::Wipe(w) => {
                commands::database::wipe(conn, w).await?;
            }
        },
        Branch(cmd) => {
            return branch::run(&cmd.subcommand, options, Some(conn), None).await;
        }
        Migrate(cmd) => {
            migrations::apply::run(cmd, conn, options).await?;
        }
        Migration(m) => match &m.subcommand {
            MigrationCmd::Apply(cmd) => {
                migrations::apply::run(cmd, conn, options).await?;
            }
            MigrationCmd::Create(cmd) => {
                migrations::create::run(cmd, conn, options).await?;
            }
            MigrationCmd::Status(params) => {
                migrations::status(conn, options, params).await?;
            }
            MigrationCmd::Log(params) => {
                migrations::log(conn, options, params).await?;
            }
            MigrationCmd::Edit(params) => {
                migrations::edit(conn, options, params).await?;
            }
            MigrationCmd::UpgradeCheck(_) => {
                anyhow::bail!("cannot be run in REPL mode");
            }
            MigrationCmd::Extract(params) => {
                migrations::extract(conn, options, params).await?;
            }
            MigrationCmd::UpgradeFormat(params) => {
                migrations::upgrade_format(conn, options, params).await?;
            }
        },
    }
    Ok(branch::CommandResult::default())
}
