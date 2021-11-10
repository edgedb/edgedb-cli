use crate::options::Options;
use crate::project::options::ProjectCommand;
use crate::server::options::ServerCommand;
use crate::server::options::ServerInstanceCommand;

use crate::portable::create;
use crate::portable::destroy;
use crate::portable::install;
use crate::portable::list_versions;
use crate::portable::project;

use crate::server::control;
use crate::server::detect;
use crate::server::info;
use crate::server::link;
use crate::server::reset_password;
use crate::server::status;
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
        List(c) => status::print_status_all(c.extended, c.debug, c.json),
        Upgrade(c) => upgrade::upgrade(c),
        cmd => control::instance_command(cmd)
    }
}

pub fn project_main(cmd: &ProjectCommand) -> anyhow::Result<()> {
    use crate::project::options::Command::*;

    use crate::project::info;
    use crate::project::unlink;
    use crate::project::upgrade;

    match &cmd.subcommand {
        Init(c) => project::init(c),
        Unlink(c) => unlink::unlink(c),
        Info(c) => info::info(c),
        Upgrade(c) => upgrade::upgrade(c),
    }
}
