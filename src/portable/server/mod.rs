mod info;
pub mod install;
mod list_versions;
mod uninstall;

use edgedb_cli_derive::IntoArgs;

use crate::portable::{repository::Channel, ver};

pub fn run(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use crate::portable::windows;
    use Command::*;

    match &cmd.subcommand {
        Install(c) if cfg!(windows) => windows::install(c),
        Install(c) => install::install(c),
        Uninstall(c) if cfg!(windows) => windows::uninstall(c),
        Uninstall(c) => uninstall::uninstall(c),
        ListVersions(c) if cfg!(windows) => windows::list_versions(c),
        ListVersions(c) => list_versions::list_versions(c),
        Info(c) if cfg!(windows) => windows::info(c),
        Info(c) => info::info(c),
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct ServerCommand {
    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    /// Show locally installed server versions.
    Info(Info),
    /// Install a server version locally.
    Install(Install),
    /// Uninstall a server version locally.
    Uninstall(Uninstall),
    /// List available and installed versions of the server.
    ListVersions(ListVersions),
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct Info {
    /// Display only the server binary path (shortcut to `--get bin-path`).
    #[arg(long)]
    pub bin_path: bool,
    /// Output in JSON format.
    #[arg(long)]
    pub json: bool,

    // Display info for latest version.
    #[arg(long)]
    #[arg(conflicts_with_all=&["channel", "version", "nightly"])]
    pub latest: bool,
    // Display info for nightly version.
    #[arg(long)]
    #[arg(conflicts_with_all=&["channel", "version", "latest"])]
    pub nightly: bool,
    // Display info for specific version.
    #[arg(long)]
    #[arg(conflicts_with_all=&["nightly", "channel", "latest"])]
    pub version: Option<ver::Filter>,
    // Display info for specific channel.
    #[arg(long, value_enum)]
    #[arg(conflicts_with_all=&["nightly", "version", "latest"])]
    pub channel: Option<Channel>,

    /// Get specific value:
    ///
    /// * `bin-path` -- Path to the server binary
    /// * `version` -- Server version
    #[arg(long, value_parser=["bin-path", "version"])]
    pub get: Option<String>,
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct Install {
    #[arg(short = 'i', long)]
    pub interactive: bool,
    #[arg(long, conflicts_with_all=&["channel", "version"])]
    pub nightly: bool,
    #[arg(long, conflicts_with_all=&["nightly", "channel"])]
    pub version: Option<ver::Filter>,
    #[arg(long, conflicts_with_all=&["nightly", "version"])]
    #[arg(value_enum)]
    pub channel: Option<Channel>,
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct Uninstall {
    /// Uninstall all versions.
    #[arg(long)]
    pub all: bool,
    /// Uninstall unused versions.
    #[arg(long)]
    pub unused: bool,
    /// Uninstall nightly versions.
    #[arg(long, conflicts_with_all=&["channel"])]
    pub nightly: bool,
    /// Uninstall specific version.
    pub version: Option<String>,
    /// Uninstall only versions from a specific channel.
    #[arg(long, conflicts_with_all=&["nightly"])]
    #[arg(value_enum)]
    pub channel: Option<Channel>,
    /// Increase verbosity.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

#[derive(clap::Args, IntoArgs, Debug, Clone)]
pub struct ListVersions {
    #[arg(long)]
    pub installed_only: bool,

    /// Single column output.
    #[arg(long, value_parser=[
        "major-version", "installed", "available",
    ])]
    pub column: Option<String>,

    /// Output in JSON format.
    #[arg(long)]
    pub json: bool,
}
