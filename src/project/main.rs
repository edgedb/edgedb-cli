use crate::project::options::{ProjectCommand, Command};

use crate::project::init;

pub fn main(cmd: &ProjectCommand) -> anyhow::Result<()> {
    use Command::*;

    match &cmd.subcommand {
        Init(c) => init::init(c),
    }
}
