use std::env;
use std::fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::print;
use crate::server::detect;
use crate::server::methods::InstallMethod;
use crate::server::options::Install;
use crate::server::version::VersionQuery;

pub mod operation;
pub mod exit_codes;
pub mod settings;


pub(in crate::server) use operation::{Operation, Command};
pub use settings::Settings;

pub const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";


fn docker_check() -> anyhow::Result<bool> {
    let cgroups = fs::read_to_string("/proc/self/cgroup")
        .context("cannot read /proc/self/cgroup")?;
    for line in cgroups.lines() {
        let mut fields = line.split(':');
        if fields.nth(2).map(|f| f.starts_with("/docker/")).unwrap_or(false) {
            return Ok(true);
        }
    }
    return Ok(false)
}

pub fn optional_docker_check() -> bool {
    if cfg!(target_os="linux") {
        let do_docker_check = env::var_os("EDGEDB_SKIP_DOCKER_CHECK")
            .map(|x| x.is_empty()).unwrap_or(true);
        if do_docker_check {
            return docker_check()
                .map_err(|e| {
                    log::warn!(
                        "Failed to check if running within a container: {:#}",
                        e,
                    )
                }).unwrap_or(false);
        }
    }
    return false;
}

pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    if optional_docker_check() {
        print::error(
            "`edgedb server install` in a Docker container is not supported.",
        );
        eprintln!("\
            To obtain a Docker image with EdgeDB server installed, \
            run the following on the host system instead:\n  \
            edgedb server install --method=docker");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let os = detect::current_os()?;
    let effective_method = options.method.clone()
        .unwrap_or(InstallMethod::Package);
    // TODO(tailhook) hint other methods on error
    let method = os.single_method(&effective_method)?;
    let version = VersionQuery::new(options.nightly, options.version.as_ref());
    let distribution = method.get_version(&version)?;

    let installed = method.installed_versions()?.into_iter()
        .find(|i| i.version_slot() == distribution.version_slot());
    if let Some(old_ver) = installed {
        print::error(format!(
            "EdgeDB {} ({}) is already installed.",
            old_ver.version_slot().title(),
            old_ver.version(),
        ));
        return Ok(());
    }

    let settings = Settings {
        method: effective_method,
        distribution,
    };
    settings.print();
    println!("Installing EdgeDB {}...", version);
    method.install(&settings)?;
    println!();
    print::success("EdgeDB is now installed. Great!");
    println!("Initialize and start a new database instance with:\n  \
          edgedb instance create{arg} <instance-name>",
          arg=if options.nightly { " --nightly" } else { "" });
    Ok(())
}
