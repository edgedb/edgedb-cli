use crate::commands::Options;
use crate::migrations::options::MigrationLog;
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::{migration, db_migration};


pub async fn log(cli: &mut Connection,
                 common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    if options.from_fs {
        log_fs_async(common, options).await
    } else if options.from_db {
        return log_db(cli, common, options).await;
    } else {
        anyhow::bail!("use either --from-fs or --from-db");
    }
}

pub async fn log_db(cli: &mut Connection, common: &Options,
    options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    let old_state = cli.set_ignore_error_state();
    let res = _log_db(cli, common, options).await;
    cli.restore_state(old_state);
    res
}

async fn _log_db(cli: &mut Connection, _common: &Options,
    options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    let migrations = db_migration::read_all(cli, false, false).await?;
    let limit = options.limit.unwrap_or(migrations.len());
    if options.newest_first {
        for rev in migrations.iter().rev().take(limit) {
            println!("{}", rev.0);
        }
    } else {
        for rev in migrations.iter().take(limit) {
            println!("{}", rev.0);
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn log_fs(common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    log_fs_async(common, options).await
}

async fn log_fs_async(_common: &Options, options: &MigrationLog)
    -> Result<(), anyhow::Error>
{
    assert!(options.from_fs);

    let ctx = Context::from_project_or_config(&options.cfg, false).await?;
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
