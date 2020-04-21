use async_std::task;

use edgeql_parser::helpers::quote_name;
use crate::options::{Options, Command};
use crate::client::{Connection, non_interactive_query};
use crate::commands;
use crate::print;
use crate::print::style::Styler;
use crate::server_params::PostgresAddress;


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
        Command::CreateDatabase(d) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                let res = cli.execute(&format!("CREATE DATABASE {}",
                                     quote_name(&d.database_name))).await?;
                print::completion(&res);
                Ok(())
            }).into()
        },
        Command::ListAliases(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_aliases(&mut cli, &cmdopt,
                    &t.pattern, t.system, t.case_sensitive, t.verbose).await?;
                Ok(())
            }).into()
        },
        Command::ListCasts(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_casts(&mut cli, &cmdopt,
                    &t.pattern, t.case_sensitive).await?;
                Ok(())
            }).into()
        },
        Command::ListIndexes(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_indexes(&mut cli, &cmdopt,
                    &t.pattern, t.system, t.case_sensitive, t.verbose).await?;
                Ok(())
            }).into()
        },
        Command::ListDatabases => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_databases(&mut cli, &cmdopt).await?;
                Ok(())
            }).into()
        },
        Command::ListPorts => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_ports(&mut cli, &cmdopt).await?;
                Ok(())
            }).into()
        },
        Command::ListScalarTypes(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_scalar_types(&mut cli, &cmdopt,
                    &t.pattern, t.system, t.case_sensitive).await?;
                Ok(())
            }).into()
        },
        Command::ListObjectTypes(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_object_types(&mut cli, &cmdopt,
                    &t.pattern, t.system, t.case_sensitive).await?;
                Ok(())
            }).into()
        },
        Command::ListRoles(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_roles(&mut cli, &cmdopt,
                    &t.pattern, t.case_sensitive).await?;
                Ok(())
            }).into()
        },
        Command::ListModules(t) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::list_modules(&mut cli, &cmdopt,
                    &t.pattern, t.case_sensitive).await?;
                Ok(())
            }).into()
        },
        Command::Pgaddr => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let cli = conn.authenticate(
                    &options, &options.database).await?;
                match cli.params.get::<PostgresAddress>() {
                    Some(addr) => {
                        println!("{}", serde_json::to_string_pretty(addr)?);
                    }
                    None => {
                        eprintln!("pgaddr requires EdgeDB to run in DEV mode");
                    }
                }
                Ok(())
            }).into()
        },
        Command::Psql => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::psql(&mut cli, &cmdopt).await?;
                Ok(())
            }).into()
        },
        Command::Describe(d) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::describe(&mut cli, &cmdopt,
                    &d.name, d.verbose).await?;
                Ok(())
            }).into()
        },
        Command::Configure(c) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::configure(&mut cli, &cmdopt, &c).await?;
                Ok(())
            }).into()
        }
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
        Command::Dump(dump) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::dump(&mut cli, &cmdopt, &dump.file.as_ref()).await?;
                Ok(())
            }).into()
        },
        Command::Restore(restore) => {
            task::block_on(async {
                let mut conn = Connection::from_options(&options).await?;
                let mut cli = conn.authenticate(
                    &options, &options.database).await?;
                commands::restore(&mut cli, &cmdopt,
                    &restore.file.as_ref(),
                    restore.allow_non_empty).await?;
                Ok(())
            }).into()
        },
    }
}
