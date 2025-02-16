mod connections;
pub mod context;
pub mod create;
pub mod current;
pub mod drop;
pub mod list;
pub mod merge;
pub mod rebase;
pub mod rename;
pub mod switch;
pub mod wipe;

use crate::branding::BRANDING;
use crate::commands::parser::BranchingCmd;
use crate::commands::Options;
use crate::connect::{Connection, Connector};
use crate::options::ConnectionOptions;
use crate::portable;

#[tokio::main(flavor = "current_thread")]
pub async fn run(options: &Options, cmd: &Command) -> anyhow::Result<CommandResult> {
    do_run(&cmd.subcommand, options, None, cmd.conn.instance.as_ref()).await
}

pub async fn do_run(
    cmd: &Subcommand,
    options: &Options,
    connection: Option<&mut Connection>,
    instance_arg: Option<&portable::options::InstanceName>,
) -> anyhow::Result<CommandResult> {
    let context = context::Context::new(instance_arg).await?;

    let mut connector: Connector = options.conn_params.clone();

    // commands that don't need existing connection
    match &cmd {
        Subcommand::Switch(switch) => return switch::run(switch, &context, &mut connector).await,
        Subcommand::Wipe(wipe) => {
            wipe::main(wipe, &context, &mut connector).await?;
            return Ok(CommandResult::default());
        }
        _ => {}
    }

    // ensure connected
    let mut conn;
    let conn_ref = if let Some(c) = connection {
        c
    } else {
        conn = Some(connector.connect().await?);
        conn.as_mut().unwrap()
    };

    verify_server_can_use_branches(conn_ref).await?;

    match cmd {
        Subcommand::Current(cmd) => current::run(cmd, &context, conn_ref).await?,
        Subcommand::Create(cmd) => create::run(cmd, &context, conn_ref).await?,
        Subcommand::Drop(cmd) => drop::main(cmd, &context, conn_ref).await?,
        Subcommand::List(cmd) => list::main(cmd, &context, conn_ref).await?,
        Subcommand::Rename(cmd) => return rename::run(cmd, &context, conn_ref, options).await,
        Subcommand::Rebase(cmd) => rebase::main(cmd, &context, conn_ref, options).await?,
        Subcommand::Merge(cmd) => merge::main(cmd, &context, conn_ref, options).await?,

        // handled earlier
        Subcommand::Switch(_) | Subcommand::Wipe(_) => unreachable!(),
    }

    Ok(CommandResult::default())
}

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    #[command(subcommand)]
    pub subcommand: Subcommand,
}

#[derive(Default)]
pub struct CommandResult {
    pub new_branch: Option<String>,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Subcommand {
    Create(create::Command),
    Switch(switch::Command),
    List(list::Command),
    Current(current::Command),
    Rebase(rebase::Command),
    Merge(merge::Command),
    Rename(rename::Command),
    Drop(drop::Command),
    Wipe(wipe::Command),
}

pub async fn verify_server_can_use_branches(connection: &mut Connection) -> anyhow::Result<()> {
    let server_version = connection.get_version().await?;
    if server_version.specific().major < 5 {
        anyhow::bail!(
            "Branches are not supported on server version {}, please upgrade to {BRANDING} 5+",
            server_version
        );
    }

    Ok(())
}

impl From<BranchingCmd> for Subcommand {
    fn from(cmd: BranchingCmd) -> Self {
        match cmd {
            BranchingCmd::Create(args) => Subcommand::Create(args),
            BranchingCmd::Drop(args) => Subcommand::Drop(args),
            BranchingCmd::Wipe(args) => Subcommand::Wipe(args),
            BranchingCmd::List(args) => Subcommand::List(args),
            BranchingCmd::Switch(args) => Subcommand::Switch(args),
            BranchingCmd::Rename(args) => Subcommand::Rename(args),
        }
    }
}
