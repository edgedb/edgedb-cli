use crate::branch::context::Context;
use crate::branch::option::{BranchCommand, Command};
use crate::branch::{create, current, drop, list, merge, rebase, rename, switch, wipe};
use crate::commands::{CommandResult, Options};
use crate::connect::{Connection, Connector};

use edgedb_tokio::get_project_dir;

#[tokio::main(flavor = "current_thread")]
pub async fn branch_main(options: &Options, cmd: &BranchCommand) -> anyhow::Result<()> {
    let context = create_context().await?;

    run_branch_command(&cmd.subcommand, options, &context, None).await?;

    Ok(())
}

pub async fn run_branch_command(
    cmd: &Command,
    options: &Options,
    context: &Context,
    connection: Option<&mut Connection>,
) -> anyhow::Result<Option<CommandResult>> {
    let mut connector: Connector = options.conn_params.clone();

    match &cmd {
        Command::Switch(switch) => return switch::main(switch, context, &mut connector).await,
        Command::Wipe(wipe) => wipe::main(wipe, context, &mut connector).await,
        Command::Current(current) => current::main(current, context).await,
        command => match connection {
            Some(conn) => return run_branch_command1(command, conn, context, options).await,
            None => {
                let mut conn = connector.connect().await?;
                return run_branch_command1(command, &mut conn, context, options).await;
            }
        },
    }?;

    Ok(None)
}

async fn run_branch_command1(
    command: &Command,
    connection: &mut Connection,
    context: &Context,
    options: &Options,
) -> anyhow::Result<Option<CommandResult>> {
    verify_server_can_use_branches(connection).await?;

    match command {
        Command::Create(create) => create::main(create, context, connection).await,
        Command::Drop(drop) => drop::main(drop, context, connection).await,
        Command::List(list) => list::main(list, context, connection).await,
        Command::Rename(rename) => return rename::main(rename, context, connection, options).await,
        Command::Rebase(rebase) => rebase::main(rebase, context, connection, options).await,
        Command::Merge(merge) => merge::main(merge, context, connection, options).await,
        unhandled => anyhow::bail!("unimplemented branch command '{:?}'", unhandled),
    }?;

    Ok(None)
}

pub async fn create_context() -> anyhow::Result<Context> {
    let project_dir = get_project_dir(None, true).await?;
    Context::new(project_dir.as_ref()).await
}

pub async fn verify_server_can_use_branches(connection: &mut Connection) -> anyhow::Result<()> {
    let server_version = connection.get_version().await?;
    if server_version.specific().major < 5 {
        anyhow::bail!(
            "Branches are not supported on server version {}, please upgrade to EdgeDB 5+",
            server_version
        );
    }

    Ok(())
}
