pub mod info;
pub mod install;
pub mod list_versions;
pub mod uninstall;

pub fn run(cmd: &Command) -> Result<(), anyhow::Error> {
    use crate::portable::windows;
    use Subcommands::*;

    match &cmd.subcommand {
        Install(c) if cfg!(windows) => windows::install(c),
        Install(c) => install::run(c),
        Uninstall(c) if cfg!(windows) => windows::uninstall(c),
        Uninstall(c) => uninstall::run(c),
        ListVersions(c) if cfg!(windows) => windows::list_versions(c),
        ListVersions(c) => list_versions::run(c),
        Info(c) if cfg!(windows) => windows::info(c),
        Info(c) => info::run(c),
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    #[command(subcommand)]
    pub subcommand: Subcommands,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Subcommands {
    /// Show locally installed server versions.
    Info(info::Command),
    /// Install a server version locally.
    Install(install::Command),
    /// Uninstall a server version locally.
    Uninstall(uninstall::Command),
    /// List available and installed versions of the server.
    ListVersions(list_versions::Command),
}
