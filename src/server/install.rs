use std::env;
use std::fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::print;
use crate::server::options::Install;
use crate::server::detect::{self, VersionQuery};
use crate::server::methods::InstallMethod;

pub mod operation;
pub mod exit_codes;
pub mod settings;


pub(in crate::server) use operation::{Operation, Command};
pub use settings::{Settings, SettingsBuilder};

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

pub fn optional_docker_check() -> anyhow::Result<bool> {
    if cfg!(target_os="linux") {
        match env::var("EDGEDB_INSTALL_IN_DOCKER").as_ref().map(|x| &x[..]) {
            Ok("forbid") | Ok("default") | Err(env::VarError::NotPresent) => {
                return Ok(docker_check()
                    .map_err(|e| {
                        log::warn!("Failed to check if running within \
                                   a container: {:#}", e)
                    }).unwrap_or(false));
            }
            Ok("allow") => return Ok(false),
            Ok(value) => {
                anyhow::bail!("Invalid value of \
                    EDGEDB_INSTALL_IN_DOCKER: {:?}. \
                    Options: allow, forbid, default.", value);
            }
            Err(env::VarError::NotUnicode(value)) => {
                anyhow::bail!("Invalid value of \
                    EDGEDB_INSTALL_IN_DOCKER: {:?}. \
                    Options: allow, forbid, default.", value);
            }
        };
    }
    return Ok(false);
}

pub fn install(options: &Install) -> Result<(), anyhow::Error> {
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
    let current_os = detect::current_os()?;
    let avail_methods = current_os.refresh_available_methods()?;
    let methods = avail_methods.instantiate_all(&*current_os, false)?;
    let effective_method = options.method.clone()
        .unwrap_or(InstallMethod::Package);
    if !options.interactive &&
        !methods.contains_key(&effective_method)
    {
        anyhow::bail!(avail_methods.format_error());
    }
    let version = VersionQuery::new(options.nightly, options.version.as_ref());
    for (meth_kind, meth) in &methods {
        for old_ver in meth.installed_versions()? {
            if version.distribution_matches(&old_ver) {
                if &effective_method == meth_kind {
                    print::error(format!(
                        "EdgeDB {} ({}) is already installed.",
                        old_ver.major_version().title(),
                        old_ver.version(),
                    ));
                    eprintln!(
                        "  Use `edgedb instance upgrade --local-minor` to \
                        upgrade local instances to the latest minor version."
                    );
                } else {
                    print::error(format!(
                        "EdgeDB {} is already installed via {}.",
                        old_ver.major_version().title(), meth_kind.option(),
                    ));
                    eprintln!("Please uninstall before installing via {}.",
                        effective_method.option());
                }
                return Err(ExitCode::new(exit_codes::ALREADY_INSTALLED))?;
            }
        }
    }
    let mut settings_builder = SettingsBuilder::new(
        &*current_os, options, methods)?;
    settings_builder.auto_version()?;
    let (settings, method) = settings_builder.build()?;
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
