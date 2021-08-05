use crate::options::Options;
use crate::server::options::{ServerCommand, Command};
use crate::server::options::{ServerInstanceCommand, InstanceCommand};

use crate::server::control;
use crate::server::create;
use crate::server::destroy;
use crate::server::detect;
use crate::server::info;
use crate::server::install;
use crate::server::link;
use crate::server::list_versions;
use crate::server::reset_password;
use crate::server::status;
use crate::server::uninstall;
use crate::server::upgrade;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        Uninstall(c) => uninstall::uninstall(c),
        ListVersions(c) => list_versions::list_versions(c),
        Info(c) => info::info(c),
        _Detect(c) => detect::main(c),
    }
}

pub fn instance_main(cmd: &ServerInstanceCommand, options: &Options) -> Result<(), anyhow::Error> {
    use InstanceCommand::*;

    match &cmd.subcommand {
        Create(c) => create::create(c),
        Destroy(c) => destroy::destroy(c),
        ResetPassword(c) => reset_password::reset_password(c),
        Link(c) => link::link(c, &options),
        List(c) => status::print_status_all(c.extended, c.debug, c.json),
        Upgrade(c) => upgrade::upgrade(c),
        cmd => control::instance_command(cmd)
    }
}
