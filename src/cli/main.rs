use crate::cli::install;
use crate::cli::migrate;
use crate::cli::options::CliCommand;
use crate::cli::options::Command;
use crate::cli::upgrade;

pub fn main(cmd: &CliCommand) -> anyhow::Result<()> {
    use Command::*;

    match &cmd.subcommand {
        Upgrade(s) => upgrade::main(s),
        Install(s) => install::main(s),
        Migrate(s) => migrate::main(s),
    }
}
