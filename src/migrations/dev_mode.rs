use edgedb_client::client::Connection;

use crate::commands::parser::Migrate;
use crate::migrations::context::Context;
use crate::migrations::create::{execute, query_row, execute_start_migration};
use crate::migrations::create::{CurrentMigration};


pub async fn migrate(cli: &mut Connection, ctx: Context, _migrate: &Migrate)
    -> anyhow::Result<()>
{
    // TODO(tailhook) first apply the whole migration history
    migrate_to_schema(cli, &ctx).await?;
    Ok(())
}

async fn migrate_to_schema(cli: &mut Connection, ctx: &Context)
    -> anyhow::Result<()>
{
    execute_start_migration(&ctx, cli).await?;
    execute(cli, "POPULATE MIGRATION").await?;
    let descr = query_row::<CurrentMigration>(cli,
        "DESCRIBE CURRENT MIGRATION AS JSON"
    ).await?;
    if !descr.complete {
        // TODO(tailhook) is `POPULATE MIGRATION` equivalent to `--yolo` or
        // should we do something manually?
        anyhow::bail!("Migration cannot be automatically populated");
    }
    if descr.confirmed.is_empty() {
        execute(cli, "ABORT MIGRATION").await?;
    } else {
        execute(cli, "COMMIT MIGRATION").await?;
    }
    Ok(())
}
