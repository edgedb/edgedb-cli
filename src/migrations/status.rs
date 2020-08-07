use async_std::stream::StreamExt;
use edgedb_protocol::value::Value;

use crate::commands::{Options, ExitCode};
use crate::commands::parser::ShowStatus;
use crate::client::Connection;
use crate::migrations::context::Context;
use crate::migrations::create::{gen_create_migration, CurrentMigration};

async fn check_migration(cli: &mut Connection, status: &ShowStatus)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<CurrentMigration>(
        "DESCRIBE CURRENT MIGRATION AS JSON",
        &Value::empty_tuple(),
    ).await?;
    while let Some(data) = items.next().await.transpose()? {
        if !data.confirmed.is_empty() || !data.proposed.is_empty() {
            if !status.quiet {
                eprintln!("Schema in database differ to on-disk schema, \
                    in particular:");
                for text in data.confirmed.iter().chain(&data.proposed).take(3)
                {
                    eprintln!("    {}",
                        text.lines().collect::<Vec<_>>()
                        .join("\n    "));
                }
                if data.confirmed.len() + data.proposed.len() > 3 {
                    eprintln!("... and {} more changes",
                        data.confirmed.len() + data.proposed.len() - 3);
                }
                eprintln!("Some migrations are missing, \
                           use `edgedb create-migration`");
            }
            return Err(ExitCode::new(1).into());
        }
    }
    Ok(())
}

pub async fn status(cli: &mut Connection, options: &Options,
    status: &ShowStatus)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_config(&status.cfg);
    // TODO(tailhook) check migration in db
    let (text, sourcemap) = gen_create_migration(&ctx).await?;
    cli.execute(text).await?;
    let check = check_migration(cli, status).await;
    let abort = cli.execute("ABORT MIGRATION").await;
    check.and(abort)?;
    Ok(())
}
