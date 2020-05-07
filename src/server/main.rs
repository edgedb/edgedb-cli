use crate::server::options::{ServerCommand, Command};
use crate::server::install;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match cmd.subcommand {
        Some(Install(ref inst)) => install::install(inst),
        None => todo!(),
    }
}
