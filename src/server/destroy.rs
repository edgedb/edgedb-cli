use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use crate::commands::{self, ExitCode};
use crate::platform::bytes_to_path;
use crate::project::init::stash_base;
use crate::server::detect;
use crate::server::errors::InstanceNotFound;
use crate::server::options::Destroy;

#[context("could not read project dir {:?}", stash_base())]
pub fn find_project_dirs(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut res = Vec::new();
    let dir = match fs::read_dir(stash_base()?) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e.into()),
    };
    for item in dir {
        let entry = item?;
        let path = entry.path().join("instance-name");
        let inst = match fs::read_to_string(&path) {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Error reading {:?}: {}", path, e);
                continue;
            }
        };
        if name == inst.trim() {
            res.push(entry.path());
        }
    }
    Ok(res)
}

#[context("cannot read {:?}", path)]
fn read_path(path: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(&path)?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}

pub fn print_warning(name: &str, project_dirs: &[PathBuf]) {
    eprintln!("Instance {:?} is used by the following projects:", name);
    for dir in project_dirs {
        let path_path = dir.join("project-path");
        let dest = match read_path(&path_path) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("edgedb error: {}", e);
                continue;
            }
        };
        eprintln!("  {}", dest.display());
    }
    eprintln!("If you really want to destroy the instance, run:");
    eprintln!("  edgedb server destroy {:?} --force", name);
}

pub fn destroy(options: &Destroy) -> anyhow::Result<()> {
    let project_dirs = find_project_dirs(&options.name)?;
    if !options.force && !project_dirs.is_empty() {
        print_warning(&options.name, &project_dirs);
        return Err(ExitCode::new(2).into());
    }
    do_destroy(options)?;
    for dir in project_dirs {
        let path_path = dir.join("project-path");
        match read_path(&path_path) {
            Ok(path) => eprintln!("Unlinking {}", path.display()),
            Err(_) => eprintln!("Cleaning {}", dir.display()),
        };
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn do_destroy(options: &Destroy) -> anyhow::Result<()> {
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let mut errors = Vec::new();
    for meth in methods.values() {
        match meth.destroy(options) {
            Ok(()) => {}
            Err(e) if e.is::<InstanceNotFound>() => {
                errors.push((meth.name(), e));
            }
            Err(e) => return Err(e),
        }
    }
    if errors.len() == methods.len() {
        eprintln!("No instances found:");
        for (meth, err) in errors {
            eprintln!("  * {}: {:#}", meth.title(), err);
        }
        Err(commands::ExitCode::new(1).into())
    } else {
        Ok(())
    }
}
