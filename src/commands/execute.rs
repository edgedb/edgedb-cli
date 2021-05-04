use edgeql_parser::helpers::quote_name;
use edgedb_client::client::Connection;
use edgedb_client::server_params::PostgresAddress;

use crate::commands::{self, Options};
use crate::commands::parser::Common;
use crate::print;
use crate::migrations;


pub async fn common(cli: &mut Connection, cmd: &Common, options: &Options)
    -> Result<(), anyhow::Error>
{
    use Common::*;
    match cmd {
        ListAliases(c) => {
            commands::list_aliases(cli, &options,
                &c.pattern, c.system, c.case_sensitive, c.verbose).await?;
        }
        ListCasts(c) => {
            commands::list_casts(cli, &options,
                &c.pattern, c.case_sensitive).await?;
        }
        ListIndexes(c) => {
            commands::list_indexes(cli, &options,
                &c.pattern, c.system, c.case_sensitive, c.verbose).await?;
        }
        ListDatabases => {
            commands::list_databases(cli, &options).await?;
        }
        ListPorts => {
            commands::list_ports(cli, &options).await?;
        }
        ListScalarTypes(c) => {
            commands::list_scalar_types(cli, &options,
                &c.pattern, c.system, c.case_sensitive).await?;
        }
        ListObjectTypes(c) => {
            commands::list_object_types(cli, &options,
                &c.pattern, c.system, c.case_sensitive).await?;
        }
        ListModules(c) => {
            commands::list_modules(cli, &options,
                &c.pattern, c.case_sensitive).await?;
        }
        ListRoles(c) => {
            commands::list_roles(cli, &options,
                &c.pattern, c.case_sensitive).await?;
        }
        Pgaddr => {
            match cli.get_param::<PostgresAddress>() {
                Some(addr) => {
                    println!("{}", serde_json::to_string_pretty(addr)?);
                }
                None => {
                    eprintln!("pgaddr requires EdgeDB to run in DEV mode");
                }
            }
        }
        Psql => {
            commands::psql(cli, &options).await?;
        }
        Describe(c) => {
            commands::describe(cli, &options, &c.name, c.verbose).await?;
        }
        DescribeSchema(_) => {
            commands::describe_schema(cli, &options).await?;
        }
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
        CreateDatabase(c) => {
            print::completion(&cli.execute(
                &format!("CREATE DATABASE {}", quote_name(&c.database_name))
            ).await?);
        }
        CreateMigration(params) => {
            migrations::create(cli, &options, params).await?;
        }
        Migrate(params) => {
            migrations::migrate(cli, &options, params).await?;
        }
        ShowStatus(params) => {
            migrations::status(cli, &options, params).await?;
        }
        MigrationLog(params) => {
            migrations::log(cli, &options, params).await?;
        }
    }
    Ok(())
}
