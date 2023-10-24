use crate::cli::install;
use crate::cli::migrate;
use crate::cli::upgrade;


#[derive(clap::Args, Clone, Debug)]
#[command(version = "help_expand")]
#[command(disable_version_flag=true)]
pub struct CliCommand {
    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    /// Upgrade the 'edgedb' command-line tool
    Upgrade(upgrade::CliUpgrade),
    /// Install the 'edgedb' command-line tool
    #[command(hide=true)]
    Install(install::CliInstall),
    /// Migrate files from `~/.edgedb` to the new directory layout
    #[command(hide=true)]
    Migrate(migrate::CliMigrate),
}
