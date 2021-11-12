use crate::options::Options;
use crate::project::options::ProjectCommand;
use crate::server::options::ServerCommand;
use crate::server::options::ServerInstanceCommand;

use crate::portable::info;
use crate::portable::control;
use crate::portable::create;
use crate::portable::destroy;
use crate::portable::install;
use crate::portable::list_versions;
use crate::portable::project;
use crate::portable::status;

use crate::server::detect;
use crate::server::link;
use crate::server::reset_password;
use crate::server::uninstall;
use crate::server::upgrade;


pub fn server_main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use crate::server::options::Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        Uninstall(c) => uninstall::uninstall(c),
        ListVersions(c) => list_versions::list_versions(c),
        Info(c) => info::info(c),
        _Detect(c) => detect::main(c),
    }
}

pub fn instance_main(cmd: &ServerInstanceCommand, options: &Options)
    -> Result<(), anyhow::Error>
{
    use crate::server::options::InstanceCommand::*;

    match &cmd.subcommand {
        Create(c) => create::create(c),
        Destroy(c) => destroy::destroy(c),
        ResetPassword(c) => reset_password::reset_password(c),
        Link(c) => link::link(c, &options),
        List(c) => status::list(c),
        Upgrade(c) => upgrade::upgrade(c),
        Start(c) => control::start(c),
        Stop(c) => control::stop(c),
        Restart(c) => control::restart(c),
        Logs(c) => control::logs(c),
        Revert(_) => todo!(),
        Unlink(_) => todo!(),
        Status(c) => status::status(c),
    }
}

pub fn project_main(cmd: &ProjectCommand) -> anyhow::Result<()> {
    use crate::project::options::Command::*;

    use crate::project::upgrade;

    match &cmd.subcommand {
        Init(c) => project::init(c),
        Unlink(c) => project::unlink(c),
        Info(c) => project::info(c),
        Upgrade(c) => upgrade::upgrade(c),
    }
}
