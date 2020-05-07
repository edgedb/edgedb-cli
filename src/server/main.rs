use crate::server::options::{ServerCommand, Command};
use crate::server::install;
use crate::server::detect;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Some(Install(c)) => install::install(c),
        Some(_Detect(c)) => detect::main(c),
        None => todo!(),
    }
}
