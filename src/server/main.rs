use crate::server::options::{ServerCommand, Command};
use crate::server::install;
use crate::server::detect;
use crate::server::list_versions;
use crate::server::init;
use crate::server::control;
use crate::server::upgrade;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        Init(c) => init::init(c),
        ListVersions(c) => list_versions::list_versions(c),
        Start(c) => control::get_instance(&c.name)?.start(c),
        Stop(c) => control::get_instance(&c.name)?.stop(c),
        Restart(c) => control::get_instance(&c.name)?.restart(c),
        Status(c) => control::get_instance(&c.name)?.status(c),
        Upgrade(c) => upgrade::upgrade(c),
        _Detect(c) => detect::main(c),
    }
}
