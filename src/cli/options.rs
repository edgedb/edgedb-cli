use edgedb_cli_derive::EdbClap;

use crate::cli::self_install;
use crate::cli::self_migrate;
use crate::cli::self_upgrade;


#[derive(EdbClap, Clone, Debug)]
pub struct SelfCommand {
    #[clap(subcommand)]
    pub subcommand: SelfSubcommand,
}

#[derive(EdbClap, Clone, Debug)]
pub enum SelfSubcommand {
    /// Upgrade this edgedb binary
    Upgrade(self_upgrade::SelfUpgrade),
    /// Install the 'edgedb' command line tool
    #[edb(hidden)]
    Install(self_install::SelfInstall),
    /// Migrate files from `~/.edgedb` to new directory layout
    #[edb(hidden)]
    Migrate(self_migrate::SelfMigrate),
}