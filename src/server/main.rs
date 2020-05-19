use crate::server::options::{ServerCommand, Command};
use crate::server::install;
use crate::server::detect;
use crate::server::list_versions;


pub fn main(cmd: &ServerCommand) -> Result<(), anyhow::Error> {
    use Command::*;

    match &cmd.subcommand {
        Install(c) => install::install(c),
        ListVersions(c) => list_versions::list_versions(c),
        _Detect(c) => detect::main(c),
    }
}
