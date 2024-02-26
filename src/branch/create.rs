use crate::branch::context::Context;
use crate::branch::option::Create;
use crate::connect::{Connection};
use crate::{portable, print};

pub async fn main(options: &Create, context: &Context, connection: &mut Connection) -> anyhow::Result<()> {
    let source_branch = options.from.as_ref().unwrap_or(&context.auto_config.current_branch);

    let query: String;

    if options.empty {
        query = format!("create empty branch {}", options.branch)
    } else {
        let branch_type = match options {
            _ if options.copy_data => "data",
            _ => "schema"
        };

        query = format!("create {} branch from {}", branch_type, source_branch)
    }

    println!("Creating branch '{}'...", options.branch);

    let status = connection.execute(
        &query,
        &()
    ).await?;

    print::completion(&status);

    if !context.update_branch(&options.branch)? {
        anyhow::bail!("Failed to update branch in edgedb.auto.toml")
    }

    Ok(())
}