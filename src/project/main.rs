use crate::project::options::{ProjectCommand, Command};

use crate::project::info;
use crate::project::init;
use crate::project::unlink;

pub fn main(cmd: &ProjectCommand) -> anyhow::Result<()> {
    use Command::*;

    match &cmd.subcommand {
        Init(c) => init::init(c),
        Unlink(c) => unlink::unlink(c),
        Info(c) => info::info(c),
    }
}
