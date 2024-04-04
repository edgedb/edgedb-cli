use crate::commands::list_databases::get_databases;
use crate::commands::{list, list_databases, Options};
use crate::connect::Connection;
use crate::print;

pub async fn get_branches(cli: &mut Connection) -> anyhow::Result<Vec<String>> {
    get_databases(cli).await
}

pub async fn list_branches(cli: &mut Connection, options: &Options)
                            -> Result<(), anyhow::Error>
{
    let version = cli.get_version().await?;

    if version.specific().major <= 4 {
        print::warn(format!("Branches are not supported in EdgeDB {}, printing list of databases instead", version));
        return list_databases(cli, options).await;
    }

    list_branches0(cli, options).await
}

pub async fn list_branches0(cli: &mut Connection, options: &Options) -> Result<(), anyhow::Error> {
    let databases = get_branches(cli).await?;
    list::print(databases, "List of branches", options).await?;
    Ok(())
}