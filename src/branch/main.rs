use crate::branch::context::Context;
use crate::branch::option::{BranchCommand, Command};
use crate::branch::{create, drop, list, merge, rebase, rename, switch, wipe};
use crate::connect::{Connection, Connector};
use crate::options::Options;

use edgedb_tokio::get_project_dir;

#[tokio::main]
pub async fn branch_main(options: &Options, cmd: &BranchCommand) -> anyhow::Result<()> {
    let context = create_context().await?;

    let mut connector: Connector = options.create_connector().await?;

    // match commands that don't require a connection to run, then match the ones that do with a connection.
    match &cmd.subcommand {
        Command::Switch(switch) => switch::main(switch, &context, &mut connector).await,
        Command::Wipe(wipe) => wipe::main(wipe, &context, &mut connector).await,
        command => {
            let mut connection = connector.connect().await?;
            verify_server_can_use_branches(&mut connection).await?;

            match command {
                Command::Create(create) => create::main(create, &context, &mut connection).await,
                Command::Drop(drop) => drop::main(drop, &context, &mut connection).await,
                Command::List(list) => list::main(list, &context, &mut connection).await,
                Command::Rename(rename) => rename::main(rename, &context, &mut connection, &options).await,
                Command::Rebase(rebase) => rebase::main(rebase, &context, &mut connection, &options).await,
                Command::Merge(merge) => merge::main(merge, &context, &mut connection, &options).await,
                unhandled => anyhow::bail!("unimplemented branch command '{:?}'", unhandled)
            }
        }
    }
}

async fn create_context() -> anyhow::Result<Context> {
    let project_dir = get_project_dir(None, true).await?.expect("Missing project");
    Context::new(&project_dir).await
}

pub async fn verify_server_can_use_branches(connection: &mut Connection) -> anyhow::Result<()> {
    let server_version = connection.get_version().await?;
    if server_version.specific().major < 5 {
        anyhow::bail!("Branches are not supported on server version {}, please upgrade to EdgeDB 5+", server_version);
    }

    Ok(())
}