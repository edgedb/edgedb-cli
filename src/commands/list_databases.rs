use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::commands::list;
use edgedb_client::client::Connection;


pub async fn list_databases(cli: &mut Connection, options: &Options)
    -> Result<(), anyhow::Error>
{
    let items = cli.query(
        "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
        &Value::empty_tuple(),
    ).await?;
    list::print(items, "List of databases", options).await?;
    Ok(())
}
