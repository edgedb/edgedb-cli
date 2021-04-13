use std::env;
use std::fs;
use std::process::exit;

use anyhow::Context;

use crate::server::options::Install;
use crate::server::detect::{self, VersionQuery};
use crate::server::methods::InstallMethod;

pub mod operation;
pub mod exit_codes;
pub mod settings;


pub(in crate::server) use operation::{Operation, Command};
pub(in crate::server) use settings::{Settings, SettingsBuilder};

pub const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";


pub fn docker_check() -> anyhow::Result<()> {
    let cgroups = fs::read_to_string("/proc/self/cgroup")
        .context("cannot read /proc/self/cgroup")?;
    for line in cgroups.lines() {
        let mut fields = line.split(':');
        if fields.nth(2).map(|f| f.starts_with("/docker/")).unwrap_or(false) {
            eprintln!("edgedb error: \
                `edgedb server install` in a Docker container is not supported.\n\
                To obtain a Docker image with EdgeDB server installed, \
                run the following on the host system instead:\n  \
                edgedb server install --method=docker");
            exit(exit_codes::DOCKER_CONTAINER);
        }
    }
    return Ok(())
}

pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    if cfg!(target_os="linux") {
        let do_docker_check = env::var_os("EDGEDB_SKIP_DOCKER_CHECK")
            .map(|x| x.is_empty()).unwrap_or(true);
        if do_docker_check {
            docker_check()
            .map_err(|e| {
                log::warn!(
                    "Failed to check if running within a container: {:#}", e)
            }).ok();
        }
    }
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
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
                    eprintln!("EdgeDB {} ({}) is already installed. \
                        Use `edgedb server upgrade` for upgrade.",
                        old_ver.major_version().title(),
                        old_ver.version());
                } else {
                    eprintln!("EdgeDB {} is already installed via {}. \
                        Please uninstall before installing via {}.",
                        old_ver.major_version().title(), meth_kind.option(),
                        effective_method.option());
                }
                exit(exit_codes::ALREADY_INSTALLED);
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
    println!("\nEdgedb server is installed now. Great!\n\
        Initialize and start a new database instance with:\n  \
          edgedb server init{arg} <instance-name>",
          arg=if options.nightly { " --nightly" } else { "" });
    Ok(())
}
