use edgedb_tokio::get_project_dir;
use crate::branch::context::Context;
use crate::branch::{create, drop, switch, wipe};
use crate::branch::option::{BranchCommand, Command};
use crate::connect::Connection;
use crate::options::Options;
use crate::portable::config;
use crate::portable::config::Config;

#[tokio::main]
pub async fn branch_main(options: &Options, cmd: &BranchCommand) -> anyhow::Result<()> {
    let context = create_context().await?;

    // match commands that don't require a connection to run, then match the ones that do with a connection.
    match &cmd.subcommand {
        Command::Switch(switch) => switch::main(switch, &context).await,
        cmd => {
            let mut connection: Connection = options.create_connector().await?.connect().await?;

            match cmd {
                Command::Create(create) => create::main(create, &context, &mut connection).await,
                Command::Drop(drop) => drop::main(drop, &context, &mut connection).await,
                Command::Wipe(wipe) => wipe::main(wipe, &context, &mut connection).await,

                _ => anyhow::bail!("Unknown subcommand")
            }
        }
    }
}

async fn create_context() -> anyhow::Result<Context> {
    let project_dir = get_project_dir(None, true).await?.expect("Missing project");
    Context::new(&project_dir)
}