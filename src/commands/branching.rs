use crate::branch;
use crate::commands::parser::BranchingCmd;
use crate::commands::{CommandResult, Options};
use crate::connect::Connection;

pub async fn main(
    connection: &mut Connection,
    cmd: &BranchingCmd,
    options: &Options,
) -> anyhow::Result<Option<CommandResult>> {
    let context = branch::context::Context::new(options).await?;
    branch::main::run_branch_command(&cmd.into(), options, &context, Some(connection)).await
}
