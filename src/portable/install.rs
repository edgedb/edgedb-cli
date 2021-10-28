use crate::portable::exit_codes;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{Channel, get_server_package};
use crate::print;
use crate::server::options::Install;


pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    if options.method.is_some() {
        return crate::server::install::install(options);
    }
    if optional_docker_check()? {
        print::error(
            "`edgedb server install` in a Docker container is not supported.",
        );
        eprintln!("\
            To obtain a Docker image with EdgeDB server installed, \
            run the following on the host system instead:\n  \
            edgedb server install --method=docker");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let channel = if options.nightly {
        Channel::Nightly
    } else {
        Channel::Stable
    };
    let ver_query = options.version.as_ref().map(|x| x.num().parse())
        .transpose().context("Unexpected --version")?;
    let pkg_info = get_server_package(channel, &ver_query)?
        .context("no package matching your criteria found")?;
    todo!();
}
