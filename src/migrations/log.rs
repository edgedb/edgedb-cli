use edgedb_client::client::Connection;

use crate::commands::Options;
use crate::commands::parser::MigrationLog;
use crate::migrations::context::Context;
use crate::migrations::migration;


pub async fn log(cli: &mut Connection, common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    if options.from_fs {
        return log_fs(common, options).await;
    }
    todo!();
}

pub async fn log_fs(_common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    assert!(options.from_fs);

    let ctx = Context::from_config(&options.cfg);
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

