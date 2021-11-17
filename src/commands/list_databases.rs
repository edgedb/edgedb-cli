use std::default::Default;

use async_std::prelude::StreamExt;
use async_std::stream::from_iter;

use crate::commands::Options;
use crate::commands::list;
use crate::server::version::Version;
use edgedb_client::client::Connection;


pub async fn get_databases<T>(cli: &mut Connection) -> anyhow::Result<T>
    where T: Default + Extend<String>,
{
    let server_ver = &cli.get_version().await?[..];
    let mut items = if Version(server_ver) < Version("1.0-alpha.6") {
        cli.query(
            "SELECT (SELECT sys::Database FILTER .name != 'edgedb0').name",
            &(),
        ).await?
    } else {
        cli.query(
            "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
            &(),
        ).await?
    };
    let mut databases = T::default();
    while let Some(name) = items.next().await.transpose()? {
        databases.extend(Some(name))
    }
    Ok(databases)
}

pub async fn list_databases(cli: &mut Connection, options: &Options)
    -> Result<(), anyhow::Error>
{
    let databases: Vec<_> = get_databases(cli).await?;
    let stream = from_iter(databases.into_iter()
        .map(|s| Ok::<_, anyhow::Error>(s)));
    list::print(stream, "List of databases", options).await?;
    Ok(())
}
