use crate::server::options::{ServerCommand, Command};
use crate::server::options::{ServerInstanceCommand, InstanceCommand};

use crate::server::control;
use crate::server::create;
use crate::server::destroy;
use crate::server::detect;
use crate::server::info;
use crate::server::install;
use crate::server::list_versions;
use crate::server::reset_password;
use crate::server::uninstall;
use crate::server::upgrade;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        Uninstall(c) => uninstall::uninstall(c),
        ListVersions(c) => list_versions::list_versions(c),
        Upgrade(c) => upgrade::upgrade(c),
        ResetPassword(c) => reset_password::reset_password(c),
        Info(c) => info::info(c),
        _Detect(c) => detect::main(c),
    }
}

pub fn instance_main(cmd: &ServerInstanceCommand) -> Result<(), anyhow::Error> {
    use InstanceCommand::{Create, Destroy};

    match &cmd.subcommand {
        Create(c) => create::create(c),
        Destroy(c) => destroy::destroy(c),
        cmd => control::instance_command(cmd)
    }
}
