use std::collections::{BTreeSet, BTreeMap};

use async_std::prelude::StreamExt;
use edgedb_client::client::Connection;
use edgedb_derive::Queryable;

use crate::commands::Options;
use crate::commands::parser::MigrationLog;
use crate::migrations::context::Context;
use crate::migrations::migration;


#[derive(Queryable, Clone)]
struct Migration {
    name: String,
    parent_names: Vec<String>,
}


pub async fn log(cli: &mut Connection, common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    if options.from_fs {
        return log_fs(common, options).await;
    } else if options.from_db {
        return log_db(cli, common, options).await;
    } else {
        anyhow::bail!("use either --from-fs or --from-db");
    }
}

fn topology_sort(migrations: Vec<Migration>) -> Vec<Migration> {
    let mut by_parent = BTreeMap::new();
    for item in &migrations {
        for parent in &item.parent_names {
            by_parent.entry(parent.clone())
                .or_insert_with(Vec::new)
                .push(item.clone());
        }
    }
    let mut output = Vec::new();
    let mut visited = BTreeSet::new();
    let mut queue = migrations.iter()
        .filter(|item| item.parent_names.is_empty())
        .map(|item| item.clone())
        .collect::<Vec<_>>();
    while let Some(item) = queue.pop() {
        output.push(item.clone());
        visited.insert(item.name.clone());
        if let Some(children) = by_parent.remove(&item.name) {
            for child in children {
                if !visited.contains(&child.name) {
                    queue.push(child.clone());
                }
            }
        }
    }
    return output
}

pub async fn log_db(cli: &mut Connection, _common: &Options,
    options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<Migration, _>(r###"
            SELECT schema::Migration {name, parent_names := .parents.name }
        "###, &()).await?;
    let mut migrations = Vec::new();
    while let Some(item) = items.next().await.transpose()? {
        migrations.push(item);
    }
    let output = topology_sort(migrations);
    let limit = options.limit.unwrap_or(output.len());
    if options.newest_first {
        for rev in output.iter().rev().take(limit) {
            println!("{}", rev.name);
        }
    } else {
        for rev in output.iter().take(limit) {
            println!("{}", rev.name);
        }
    }
    Ok(())
}

pub async fn log_fs(_common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    assert!(options.from_fs);

    let ctx = Context::from_project_or_config(&options.cfg)?;
    let migrations = migration::read_all(&ctx, true).await?;
    let limit = options.limit.unwrap_or(migrations.len());
    if options.newest_first {
        for rev in migrations.keys().rev().take(limit) {
            println!("{}", rev);
        }
    } else {
        for rev in migrations.keys().take(limit) {
            println!("{}", rev);
        }
    }
    Ok(())
}
