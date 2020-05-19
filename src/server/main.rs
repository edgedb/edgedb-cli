use crate::server::options::{ServerCommand, Command};
use crate::server::install;
use crate::server::detect;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        _Detect(c) => detect::main(c),
    }
}
