use edgeql_parser::helpers::quote_name;
use crate::connect::Connection;
use edgedb_tokio::server_params::PostgresAddress;

use crate::commands::{self, Options};
use crate::commands::parser::{Common, DatabaseCmd, MigrationCmd};
use crate::commands::parser::{ListCmd, DescribeCmd};
use crate::print;
use crate::migrations;


pub async fn common(cli: &mut Connection, cmd: &Common, options: &Options)
    -> Result<(), anyhow::Error>
{
    use Common::*;
    match cmd {
        List(c) => match &c.subcommand {
            ListCmd::Aliases(c) => {
                commands::list_aliases(cli, &options,
                    &c.pattern, c.system, c.case_sensitive, c.verbose).await?;
            }
            ListCmd::Casts(c) => {
                commands::list_casts(cli, &options,
                    &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Indexes(c) => {
                commands::list_indexes(cli, &options,
                    &c.pattern, c.system, c.case_sensitive, c.verbose).await?;
            }
            ListCmd::Databases => {
                commands::list_databases(cli, &options).await?;
            }
            ListCmd::Scalars(c) => {
                commands::list_scalar_types(cli, &options,
                    &c.pattern, c.system, c.case_sensitive).await?;
            }
            ListCmd::Types(c) => {
                commands::list_object_types(cli, &options,
                    &c.pattern, c.system, c.case_sensitive).await?;
            }
            ListCmd::Modules(c) => {
                commands::list_modules(cli, &options,
                    &c.pattern, c.case_sensitive).await?;
            }
            ListCmd::Roles(c) => {
                commands::list_roles(cli, &options,
                    &c.pattern, c.case_sensitive).await?;
            }
        }
        Pgaddr => {
            match cli.get_server_param::<PostgresAddress>() {
                Some(addr) => {
                    println!("{}", serde_json::to_string_pretty(addr)?);
                }
                None => {
                    print::error("pgaddr requires EdgeDB to run in DEV mode");
                }
            }
        }
        Psql => {
            commands::psql(cli, &options).await?;
        }
        Describe(c) => match &c.subcommand {
            DescribeCmd::Object(c) => {
                commands::describe(cli, &options, &c.name, c.verbose).await?;
            }
            DescribeCmd::Schema(_) => {
                commands::describe_schema(cli, &options).await?;
            }
        },
        Dump(c) => {
            commands::dump(cli, &options, c).await?;
        }
        Restore(params) => {
            commands::restore(cli, &options, params)
            .await?;
        }
        Configure(c) => {
            commands::configure(cli, &options, c).await?;
        }
        Database(c) => match &c.subcommand {
            DatabaseCmd::Create(c) => {
                print::completion(&cli.execute(
                    &format!("CREATE DATABASE {}",
                             quote_name(&c.database_name)),
                    &(),
                ).await?);
            }
        }
        Migrate(params) => {
            migrations::migrate(cli, &options, params).await?;
        }
        Migration(m) => match &m.subcommand {
            MigrationCmd::Apply(params) => {
                migrations::migrate(cli, &options, params).await?;
            }
            MigrationCmd::Create(params) => {
                migrations::create(cli, &options, params).await?;
            }
            MigrationCmd::Status(params) => {
                migrations::status(cli, &options, params).await?;
            }
            MigrationCmd::Log(params) => {
                migrations::log_async(cli, &options, params).await?;
            }
            MigrationCmd::Edit(params) => {
                migrations::edit(cli, &options, params).await?;
            }
        }
    }
    Ok(())
}
