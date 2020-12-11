use async_std::prelude::StreamExt;
use async_std::stream::from_iter;

use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::commands::list;
use crate::server::version::Version;
use edgedb_client::client::Connection;


pub async fn get_databases(cli: &mut Connection)
    -> Result<Vec<String>, anyhow::Error>
{
    let server_ver = &cli.get_version().await?[..];
    let mut items = if Version(server_ver) < Version("1.0-alpha.6") {
        cli.query(
            "SELECT (SELECT sys::Database FILTER .name != 'edgedb0').name",
            &Value::empty_tuple(),
        ).await?
    } else {
        cli.query(
            "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
            &Value::empty_tuple(),
        ).await?
    };
    let mut databases: Vec<String> = Vec::new();
    while let Some(name) = items.next().await.transpose()? {
        databases.push(name)
    }
    Ok(databases)
}

pub async fn list_databases(cli: &mut Connection, options: &Options)
    -> Result<(), anyhow::Error>
{
    let databases = get_databases(cli).await?;
    let stream = from_iter(databases.into_iter()
        .map(|s| Ok::<_, anyhow::Error>(s)));
    list::print(stream, "List of databases", options).await?;
    Ok(())
}
