use async_std::prelude::StreamExt;

use prettytable::{Table, Row, Cell};

use edgedb_derive::Queryable;
use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::client::Client;
use crate::table;



#[derive(Queryable)]
struct PortRow {
    addresses: String,
    concurrency: i64,
    database: String,
    port: i64,
    protocol: String,
    user: String,
}

pub async fn list_ports<'x>(cli: &mut Client<'x>, options: &Options)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<PortRow>(r###"
        SELECT cfg::Port {
            addresses := to_str(array_agg(.address), ', '),
            concurrency,
            database,
            port,
            protocol,
            user,
        }
    "###, &Value::empty_tuple()).await?;
    if !options.command_line || atty::is(atty::Stream::Stdout) {
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Port", "Protocol", "Concurrency",
             "Database", "User", "Addresses"]
            .iter().map(|x| table::header_cell(x)).collect()));
        while let Some(item) = items.next().await.transpose()? {
            table.add_row(Row::new(vec![
                Cell::new(&item.port.to_string()),
                Cell::new(&item.protocol),
                Cell::new(&item.concurrency.to_string()),
                Cell::new(&item.database),
                Cell::new(&item.user),
                Cell::new(&item.addresses),
            ]));
        }
        if table.is_empty() {
            eprintln!("No ports defined");
        } else {
            table.printstd();
        }
    } else {
        while let Some(item) = items.next().await.transpose()? {
            println!("{}\t{}\t{}\t{}\t{}\t{}",
                item.port, item.protocol, item.concurrency,
                item.database, item.user, item.addresses);
        }
    }
    Ok(())
}
