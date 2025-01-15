use crate::options::Options;
use crate::portable::project::ProjectCommand;

use crate::portable::project;

pub fn project_main(cmd: &ProjectCommand, options: &Options) -> anyhow::Result<()> {
    use crate::portable::project::Command::*;

    match &cmd.subcommand {
        Init(c) => project::init(c, options),
        Unlink(c) => project::unlink(c, options),
        Info(c) => project::info(c),
        Upgrade(c) => project::upgrade(c, options),
    }
}
