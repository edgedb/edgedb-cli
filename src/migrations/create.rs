use async_std::path::{Path, PathBuf};
use async_std::fs;
use async_std::stream::StreamExt;
use fn_error_context::context;
use edgedb_derive::Queryable;
use edgedb_protocol::value::Value;
use edgeql_parser::preparser::{full_statement, is_empty};
use serde::Deserialize;

use crate::commands::Options;
use crate::commands::parser::CreateMigration;
use crate::client::Connection;
use crate::migrations::context::Context;
use crate::migrations::sourcemap::{Builder, SourceMap};

pub enum SourceName {
    Prefix,
    Suffix,
    File(PathBuf),
}

#[derive(Deserialize, Queryable, Debug)]
#[edgedb(json)]
pub struct CurrentMigration {
    pub confirmed: Vec<String>,
    pub proposed: Vec<String>,
}

#[context("error reading schema file {}", path.display())]
async fn read_schema_file(path: &Path) -> anyhow::Result<String> {
    let data = fs::read_to_string(path).await?;
    let mut offset = 0;
    loop {
        match full_statement(data[offset..].as_bytes(), None) {
            Ok(shift) => offset += shift,
            Err(_) => {
                if !is_empty(&data[offset..]) {
                    anyhow::bail!("Last statement must end with semicolon");
                }
                return Ok(data);
            }
        }
    }
}

#[context("error reading schema at {}", ctx.schema_dir.display())]
pub async fn gen_create_migration(ctx: &Context)
    -> anyhow::Result<(String, SourceMap<SourceName>)>
{
    let mut bld = Builder::new();
    bld.add_lines(SourceName::Prefix, "START MIGRATION TO {");
    let mut dir = fs::read_dir(&ctx.schema_dir).await?;
    while let Some(item) = dir.next().await.transpose()? {
        let fname = item.file_name();
        let lossy_name = fname.to_string_lossy();
        if lossy_name.starts_with(".") || !lossy_name.ends_with(".esdl")
            || !item.file_type().await?.is_file()
        {
            continue;
        }
        let path = item.path();
        let chunk = read_schema_file(&path).await?;
        bld.add_lines(SourceName::File(path), &chunk);
    }
    bld.add_lines(SourceName::Suffix, "};");
    Ok(bld.done())
}


pub async fn create(cli: &mut Connection, options: &Options,
    create: &CreateMigration)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_config(&create.cfg);
    let (text, sourcemap) = gen_create_migration(&ctx).await?;
    cli.execute(text).await?;
    let mut data = cli.query_row::<CurrentMigration>(
        "DESCRIBE CURRENT MIGRATION AS JSON",
        &Value::empty_tuple(),
    ).await?;
    println!("DATA {:?}", data);
    Ok(())
}
