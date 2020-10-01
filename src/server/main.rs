use crate::server::options::{ServerCommand, Command};
use crate::server::install;
use crate::server::destroy;
use crate::server::detect;
use crate::server::list_versions;
use crate::server::init;
use crate::server::control;
use crate::server::upgrade;
use crate::server::reset_password;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        Init(c) => init::init(c),
        Destroy(c) => destroy::destroy(c),
        ListVersions(c) => list_versions::list_versions(c),
        Instance(c) => control::instance_command(c),
        Upgrade(c) => upgrade::upgrade(c),
        ResetPassword(c) => reset_password::reset_password(c),
        _Detect(c) => detect::main(c),
    }
}
