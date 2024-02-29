use std::ops::Deref;
use anyhow::Error;
use crate::branch::context::Context;
use crate::connect::Connection;
use crate::options::Options;
use uuid::Uuid;

pub struct BranchConnection<'a> {
    pub connection: Connection,
    branch_name: String,
    is_temp: bool,
    options: &'a Options
}

impl BranchConnection<'_> {
    pub async fn clean(self) -> anyhow::Result<()> {
        if self.is_temp {
            let mut branch = get_connection_that_is_not(
                &self.branch_name,
                self.options,
                &mut self.options.create_connector().await?.connect().await?
            ).await?;

            branch.connection.execute(&format!("drop branch {}", self.branch_name), &()).await?;

            // should never happen, but just to make sure
            if branch.is_temp {
                anyhow::bail!("Cannot create a temp branch to remove a temp branch");
            }
        }

        Ok(())
    }
}


pub async fn get_connection_to_modify<'a>(branch: &String, options: &'a Options, connection: &mut Connection) -> anyhow::Result<BranchConnection<'a>> {
    match get_connection_that_is_not(branch, options, connection).await {
        Ok(connection) => Ok(connection),
        Err(_) => {
            let temp_name = Uuid::new_v4().to_string();
            connection.execute(&format!("create empty branch {}", temp_name), &()).await?;
            Ok(BranchConnection {
                connection: options.create_connector().await?.database(&temp_name)?.connect().await?,
                options,
                branch_name: temp_name,
                is_temp: true
            })
        }
    }
}

pub async fn get_connection_that_is_not<'a>(target_branch: &String, options: &'a Options, connection: &mut Connection) -> anyhow::Result<BranchConnection<'a>> {
    let branches: Vec<String> = connection.query(
        "SELECT (SELECT sys::Database FILTER NOT .builtin).name",
        &(),
    ).await?;

    for branch in &branches {
        if branch != target_branch {
            return Ok(BranchConnection {
                connection: options.create_connector().await?.database(branch)?.connect().await?,
                branch_name: branch.deref().to_string(),
                is_temp: false,
                options
            }.into())
        }
    }

    anyhow::bail!("Cannot find other branches to use");
}