use edgedb_cli_derive::EdbClap;

use crate::cli::cli_install;
use crate::cli::cli_migrate;
use crate::cli::cli_upgrade;


#[derive(EdbClap, Clone, Debug)]
pub struct CliCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Upgrade this edgedb binary
    Upgrade(cli_upgrade::CliUpgrade),
    /// Install the 'edgedb' command line tool
    #[edb(hidden)]
    Install(cli_install::CliInstall),
    /// Migrate files from `~/.edgedb` to new directory layout
    #[edb(hidden)]
    Migrate(cli_migrate::CliMigrate),
}
