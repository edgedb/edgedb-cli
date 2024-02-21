use crate::branch::context::Context;
use crate::branch::option::Switch;
use crate::connect::Connection;

pub async fn main(options: &Switch, context: &Context) -> anyhow::Result<()> {
    // TODO: Do we verify whether the target branch exists? seems like a possible softlock if we don't: if we're on a
    // branch that doesn't exist then any attempts to connect to it will fail softlocking commands that require
    // connecting

    println!("Switching from '{}' to '{}'", context.auto_config.current_branch, options.branch);

    if !context.update_branch(&options.branch)? {
        anyhow::bail!("Failed to update branch in edgedb.auto.toml")
    }

    println!("Now on '{}'", options.branch);

    Ok(())
}