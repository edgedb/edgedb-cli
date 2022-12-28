use std::default::Default;

use crate::commands::Options;
use crate::commands::list;
use crate::connect::Connection;


pub async fn get_databases(cli: &mut Connection) -> anyhow::Result<Vec<String>>
{
    let databases = cli.query(
        "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
        &(),
    ).await?;
    Ok(databases)
}

pub async fn list_databases(cli: &mut Connection, options: &Options)
    -> Result<(), anyhow::Error>
{
    let databases = get_databases(cli).await?;
    list::print(databases, "List of databases", options).await?;
    Ok(())
}
