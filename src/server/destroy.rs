use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use crate::commands;
use crate::project::init::stash_base;
use crate::print;
use crate::server::detect;
use crate::server::errors::InstanceNotFound;
use crate::server::options::Destroy;
use crate::platform::{bytes_to_path};


#[context("could not read project dir {:?}", stash_base())]
pub fn find_project_dirs(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut res = Vec::new();
    let dir = match fs::read_dir(stash_base()?) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e)?,
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

pub fn print_instance_in_use_warning(name: &str, project_dirs: &[PathBuf]) {
    print::warn(format!(
        "Instance {:?} is used by the following project{}:",
        name,
        if project_dirs.len() > 1 { "s" } else { "" },
    ));
    for dir in project_dirs {
        let dest = match read_project_real_path(dir) {
            Ok(path) => path,
            Err(e) => {
                print::error(e);
                continue;
            }
        };
        eprintln!("  {}", dest.display());
    }
}

pub fn do_destroy(mut errors: Vec<(String, anyhow::Error)>, options: &Destroy)
    -> anyhow::Result<()>
{
    let prev_errors = errors.len();
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    for meth in methods.values() {
        match meth.destroy(options) {
            Ok(()) => {}
            Err(e) if e.is::<InstanceNotFound>() => {
                errors.push((meth.name().title().to_string(), e));
            }
            Err(e) => Err(e)?,
        }
    }
    if prev_errors > 0 && errors.len() == methods.len() + prev_errors {
        print::error("No instances found:");
        for (meth, err) in errors {
            eprintln!("  * {}: {:#}", meth, err);
        }
        Err(commands::ExitCode::new(1).into())
    } else {
        Ok(())
    }
}

#[context("cannot read {:?}", project_dir)]
pub fn read_project_real_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(&project_dir.join("project-path"))?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}
