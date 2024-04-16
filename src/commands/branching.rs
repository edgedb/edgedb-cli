use crate::{branch};
use crate::commands::parser::BranchingCmd;
use crate::connect::Connection;
use crate::commands::{CommandResult, Options};


pub async fn main(connection: &mut Connection, cmd: &BranchingCmd, options: &Options) -> anyhow::Result<Option<CommandResult>> {
    let context = branch::main::create_context().await?;
    branch::main::run_branch_command(&cmd.into(), options, &context, Some(connection)).await
}