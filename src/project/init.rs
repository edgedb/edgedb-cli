use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::project::options::Init;
use crate::question;
use crate::server::methods::{InstallMethod, InstallationMethods};
use crate::server::detect;


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

pub fn init(init: &Init) -> anyhow::Result<()> {
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
    todo!();
}
