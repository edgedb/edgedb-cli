use edgedb_cli_derive::EdbClap;

use crate::cli::install;
use crate::cli::migrate;
use crate::cli::upgrade;


#[derive(EdbClap, Clone, Debug)]
pub struct CliCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Upgrade this edgedb binary
    Upgrade(upgrade::CliUpgrade),
    /// Install the 'edgedb' command line tool
    #[edb(hidden)]
    Install(install::CliInstall),
    /// Migrate files from `~/.edgedb` to new directory layout
    #[edb(hidden)]
    Migrate(migrate::CliMigrate),
}
