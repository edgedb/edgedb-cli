use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::BTreeSet;

use anyhow::Context;
use linked_hash_map::LinkedHashMap;

use crate::commands::ExitCode;
use crate::project::options::Init;
use crate::question;
use crate::server::detect::{self, VersionQuery};
use crate::server::distribution::DistributionRef;
use crate::server::install::{self, optional_docker_check, exit_codes};
use crate::server::methods::{InstallMethod, InstallationMethods};
use crate::server::os_trait::Method;
use crate::server::version::Version;


fn config_dir(base: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut path = base;
    if path.join("edgedb.toml").exists() {
        return Ok(Some(path.into()));
    }
    while let Some(parent) = path.parent() {
        if parent.join("edgedb.toml").exists() {
            return Ok(Some(parent.into()));
        }
        path = parent;
    }
    Ok(None)
}

fn ask_method(available: &InstallationMethods) -> anyhow::Result<InstallMethod>
{
    let mut q = question::Numeric::new(
        "What type of EdgeDB instance would you like to use with this project?"
    );
    if available.package.supported {
        q.option("Local (native package)", InstallMethod::Package);
    }
    if available.package.supported {
        q.option("Local (docker)", InstallMethod::Docker);
    }
    q.ask()
}

fn ask_version(meth: &Method) -> anyhow::Result<DistributionRef> {
    let distribution = meth.get_version(&VersionQuery::Stable(None))
        .context("cannot find stable EdgeDB version")?;
    let mut q = question::String::new(
        "Specify the version of EdgeDB to use with this project"
    );
    q.default(distribution.major_version().as_str());
    loop {
        let value = q.ask()?;
        let value = value.trim();
        if value == distribution.major_version().as_str() {
            return Ok(distribution);
        }
        if value == "nightly" {
            match meth.get_version(&VersionQuery::Nightly) {
                Ok(distr) => return Ok(distr),
                Err(e) => {
                    eprintln!("Cannot find nightly version: {}", e);
                    continue;
                }
            }
        } else {
            let query = VersionQuery::Stable(Some(Version(value.into())));
            match meth.get_version(&query) {
                Ok(distr) => return Ok(distr),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    let mut avail = meth.all_versions(false)?;
                    avail.sort_by(|a, b| {
                        b.major_version().cmp(a.major_version())
                    });
                    println!("Available versions: {}{}",
                        avail.iter().take(5)
                        .map(|d| d.major_version().as_str().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                        if avail.len() > 5 { " ..." } else { "" });
                    continue;
                }
            }
        }
    }
}

pub fn init(init: &Init) -> anyhow::Result<()> {
    if optional_docker_check() {
        eprintln!("edgedb error: \
            `edgedb project init` in a Docker container is not supported.\n\
            To init project run the command on the host system instead and \
            choose `Local (docker)` installation method.");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let dir = match &init.project_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir()
            .context("failed to get current directory")?,
    };
    if let Some(dir) = config_dir(&dir)? {
        let dir = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        init_existing(init, &dir)?;
    } else {
        let dir = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        init_new(init, &dir)?;
    }
    Ok(())
}

fn init_existing(init: &Init, project_dir: &Path) -> anyhow::Result<()> {
    todo!();
}

fn init_new(init: &Init, project_dir: &Path) -> anyhow::Result<()> {
    println!("`edgedb.toml` is not found in `{}` or above",
             project_dir.display());
    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let method = ask_method(&avail_methods)?;
    let methods = avail_methods.instantiate_all(&*os, true)?;
    let instances = methods.values()
        .map(|m| m.all_instances())
        .collect::<Result<Vec<_>, _>>()
        .context("failed to enumerate existing instances")?
        .into_iter().flatten()
        .map(|inst| inst.name().to_string())
        .collect::<BTreeSet<_>>();
    let meth = methods.get(&method).expect("chosen method works");
    let installed = meth.installed_versions()?;
    let distr = ask_version(meth.as_ref())?;

    // TODO(tailhook) this condition doesn't work for nightly
    if !installed.iter().any(|x| x.major_version() == distr.major_version()) {
        meth.install(&install::Settings {
            method,
            distribution: distr,
            extra: LinkedHashMap::new(),
        })?;
    }
    todo!();
}
