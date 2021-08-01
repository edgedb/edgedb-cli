use crate::cli::cli_install;
use crate::cli::cli_migrate;
use crate::cli::cli_upgrade;
use crate::cli::options::CliCommand;
use crate::cli::options::Command;


pub fn main(cmd: &CliCommand) -> anyhow::Result<()> {
    use Command::*;

    match &cmd.subcommand {
        Upgrade(s) => cli_upgrade::main(s),
        Install(s) => cli_install::main(s),
        Migrate(s) => cli_migrate::main(s),
    }
}