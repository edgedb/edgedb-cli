use crate::branch::context::Context;
use crate::branch::option::{BranchCommand, Command};
use crate::branch::{create, drop, list, rebase, rename, switch, wipe};
use crate::connect::Connection;
use crate::options::Options;
use crate::portable::config::Config;
use edgedb_tokio::get_project_dir;

#[tokio::main]
pub async fn branch_main(options: &Options, cmd: &BranchCommand) -> anyhow::Result<()> {
    let context = create_context().await?;

    let mut connection: Connection = options.create_connector().await?.connect().await?;

    // match commands that don't require a connection to run, then match the ones that do with a connection.
    match &cmd.subcommand {
        Command::Switch(switch) => switch::main(switch, &context, &mut connection).await,
        Command::Create(create) => create::main(create, &context, &mut connection).await,
        Command::Drop(drop) => drop::main(drop, &context, &mut connection).await,
        Command::Wipe(wipe) => wipe::main(wipe, &context, &mut connection).await,
        Command::List(list) => list::main(list, &context, &mut connection).await,
        Command::Rename(rename) => rename::main(rename, &context, &mut connection).await,
        Command::Rebase(rebase) => rebase::main(rebase, &context, &mut connection, &options).await,
    }
}

async fn create_context() -> anyhow::Result<Context> {
    let project_dir = get_project_dir(None, true).await?.expect("Missing project");
    Context::new(&project_dir)
}
