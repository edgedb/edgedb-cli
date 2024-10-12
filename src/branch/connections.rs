use std::ops::Deref;

use crate::commands::Options;
use crate::connect::{Connection, Connector};
use crate::print;
use uuid::Uuid;

pub struct BranchConnection<'a> {
    pub connection: Connection,
    branch_name: String,
    is_temp: bool,
    options: &'a Options,
}

impl BranchConnection<'_> {
    pub async fn clean(self) -> anyhow::Result<()> {
        if self.is_temp {
            let mut branch = get_connection_that_is_not(
                &self.branch_name,
                self.options,
                &mut self.options.conn_params.connect().await?,
            )
            .await?;

            let (status, _warnings) = branch
                .connection
                .execute(&format!("drop branch {} force", self.branch_name), &())
                .await?;
            print::completion(status);

            // should never happen, but just to make sure
            if branch.is_temp {
                anyhow::bail!("Cannot create a temp branch to remove a temp branch");
            }
        }

        Ok(())
    }
}

/// Attempts to connect a provided connection, returning `Ok(Some)` if the connection was
/// established, `Ok(None)` if the connection couldn't be established because of a
/// `ClientConnectionFailedError`, and `Err` if any other type of error occurred.
pub async fn connect_if_branch_exists(connector: &Connector) -> anyhow::Result<Option<Connection>> {
    match connector.connect().await {
        Ok(c) => Ok(Some(c)),
        Err(e) => {
            match e.downcast::<edgedb_tokio::Error>() {
                Ok(e) => {
                    if e.code() == 0x_FF_01_01_00 {
                        // 0x_FF_01_01_00: ClientConnectionFailedError | https://www.edgedb.com/docs/reference/protocol/errors
                        return Ok(None);
                    }

                    Err(e.into())
                }
                Err(e) => Err(e),
            }
        }
    }
}

pub async fn get_connection_to_modify<'a>(
    branch: &str,
    options: &'a Options,
    connection: &mut Connection,
) -> anyhow::Result<BranchConnection<'a>> {
    match get_connection_that_is_not(branch, options, connection).await {
        Ok(connection) => Ok(connection),
        Err(_) => {
            let temp_name = Uuid::new_v4().to_string();
            connection
                .execute(
                    &format!(
                        "create empty branch {}",
                        edgeql_parser::helpers::quote_name(&temp_name)
                    ),
                    &(),
                )
                .await?;

            let mut conn_params = options.conn_params.clone();
            Ok(BranchConnection {
                connection: conn_params.branch(&temp_name)?.connect().await?,
                options,
                branch_name: temp_name,
                is_temp: true,
            })
        }
    }
}

pub async fn get_connection_that_is_not<'a>(
    target_branch: &str,
    options: &'a Options,
    connection: &mut Connection,
) -> anyhow::Result<BranchConnection<'a>> {
    let branches: Vec<String> = connection
        .query(
            "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
            &(),
        )
        .await?;

    for branch in &branches {
        if branch != target_branch {
            let mut connector = options.conn_params.clone();
            return Ok(BranchConnection {
                connection: connector.branch(branch)?.connect().await?,
                branch_name: branch.deref().to_string(),
                is_temp: false,
                options,
            });
        }
    }

    anyhow::bail!("Cannot find other branches to use");
}
