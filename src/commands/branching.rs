use crate::branch;
use crate::commands::parser::BranchingCmd;
use crate::commands::Options;
use crate::connect::Connection;

pub async fn main(
    connection: &mut Connection,
    cmd: &BranchingCmd,
    options: &Options,
) -> anyhow::Result<()> {
    let context = branch::main::create_context().await?;

    branch::main::run_branch_command(&cmd.into(), options, &context, Some(connection)).await
}
