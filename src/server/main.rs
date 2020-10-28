use crate::server::options::{ServerCommand, Command};

use crate::server::control;
use crate::server::destroy;
use crate::server::detect;
use crate::server::init;
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
        Init(c) => init::init(c),
        Destroy(c) => destroy::destroy(c),
        ListVersions(c) => list_versions::list_versions(c),
        Instance(c) => control::instance_command(c),
        Upgrade(c) => upgrade::upgrade(c),
        ResetPassword(c) => reset_password::reset_password(c),
        _Detect(c) => detect::main(c),
    }
}
