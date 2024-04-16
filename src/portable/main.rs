use crate::options::Options;
use crate::portable::project::ProjectCommand;
use crate::portable::options::{ServerCommand, ServerInstanceCommand};

use crate::portable::control;
use crate::portable::create;
use crate::portable::credentials;
use crate::portable::destroy;
use crate::portable::info;
use crate::portable::install;
use crate::portable::link;
use crate::portable::list_versions;
use crate::portable::project;
use crate::portable::resize;
use crate::portable::revert;
use crate::portable::status;
use crate::portable::uninstall;
use crate::portable::upgrade;
use crate::portable::reset_password;
use crate::portable::windows;


pub fn server_main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use crate::portable::options::Command::*;

    match &cmd.subcommand {
        Install(c) if cfg!(windows) => windows::install(c),
        Install(c) => install::install(c),
        Uninstall(c) if cfg!(windows) => windows::uninstall(c),
        Uninstall(c) => uninstall::uninstall(c),
        ListVersions(c) if cfg!(windows) => windows::list_versions(c),
        ListVersions(c) => list_versions::list_versions(c),
        Info(c) if cfg!(windows) => windows::info(c),
        Info(c) => info::info(c),
    }
}

pub fn instance_main(cmd: &ServerInstanceCommand, options: &Options)
    -> Result<(), anyhow::Error>
{
    use crate::portable::options::InstanceCommand::*;

    match &cmd.subcommand {
        Create(c) => create::create(c, options),
        Destroy(c) => destroy::destroy(c, options),
        ResetPassword(c) => reset_password::reset_password(c),
        Link(c) => link::link(c, options),
        List(c) if cfg!(windows) => windows::list(c, options),
        List(c) => status::list(c, options),
        Resize(c) => resize::resize(c, options),
        Upgrade(c) => upgrade::upgrade(c, options),
        Start(c) => control::start(c),
        Stop(c) => control::stop(c),
        Restart(c) if cfg!(windows) => windows::restart(c),
        Restart(c) => control::restart(c),
        Logs(c) if cfg!(windows) => windows::logs(c),
        Logs(c) => control::logs(c),
        Revert(c) => revert::revert(c),
        Unlink(c) => link::unlink(c),
        Status(c) if cfg!(windows) => windows::status(c),
        Status(c) => status::status(c, options),
        Credentials(c) => credentials::show_credentials(options, c),
    }
}

pub fn project_main(cmd: &ProjectCommand, options: &Options) -> anyhow::Result<()> {
    use crate::portable::project::Command::*;

    match &cmd.subcommand {
        Init(c) => project::init(c, options),
        Unlink(c) => project::unlink(c, options),
        Info(c) => project::info(c),
        Upgrade(c) => project::upgrade(c, options),
    }
}
