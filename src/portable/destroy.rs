use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use crate::commands::ExitCode;
use crate::platform::bytes_to_path;
use crate::portable::exit_codes;
use crate::portable::local;
use crate::portable::project::stash_base;
use crate::portable::{windows, linux, macos};
use crate::print;
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

pub fn print_warning(name: &str, project_dirs: &[PathBuf]) {
    print_instance_in_use_warning(name, project_dirs);
    eprintln!("If you really want to destroy the instance, run:");
    eprintln!("  edgedb instance destroy {:?} --force", name);
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

#[context("cannot read {:?}", project_dir)]
pub fn read_project_real_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(&project_dir.join("project-path"))?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}

pub fn destroy(options: &Destroy) -> anyhow::Result<()> {
    let project_dirs = find_project_dirs(&options.name)?;
    if !options.force && !project_dirs.is_empty() {
        print_warning(&options.name, &project_dirs);
        return Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
    }
    do_destroy(options)?;
    for dir in project_dirs {
        match read_project_real_path(&dir) {
            Ok(path) => eprintln!("Unlinking {}", path.display()),
            Err(_) => eprintln!("Cleaning {}", dir.display()),
        };
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn stop_and_disable(name: &str) -> anyhow::Result<bool> {
    if cfg!(target_os="macos") {
        macos::stop_and_disable(&name)
    } else if cfg!(target_os="linux") {
        linux::stop_and_disable(&name)
    } else if cfg!(windows) {
        windows::stop_and_disable(&name)
    } else {
        anyhow::bail!("service is not supported on the platform");
    }
}

fn destroy_portable(options: &Destroy) -> anyhow::Result<()> {
    let paths = local::Paths::get(&options.name)?;
    let mut found = false;
    let mut not_found_err = None;
    match stop_and_disable(&options.name) {
        Ok(f) => found = f,
        Err(e) if e.is::<InstanceNotFound>() => {
            not_found_err.insert(e);
        }
        Err(e) => {
            log::warn!("Error unloading service: {:#}", e);
        }
    }
    if paths.data_dir.exists() {
        found = true;
        log::info!("Removing data directory {:?}", paths.data_dir);
        fs::remove_dir_all(&paths.data_dir)?;
    }
    if paths.credentials.exists(){
        found = true;
        log::info!("Removing credentials file {:?}", &paths.credentials);
        fs::remove_file(&paths.credentials)?;
    }
    for path in &paths.service_files {
        if path.exists() {
            found = true;
            log::info!("Removing service file {:?}", path);
            fs::remove_file(path)?;
        }
    }
    if found {
        Ok(())
    } else if let Some(e) = not_found_err {
        Err(e)
    } else {
        Err(InstanceNotFound(anyhow::anyhow!("instance not found")).into())
    }
}

pub fn do_destroy(options: &Destroy) -> anyhow::Result<()> {
    match destroy_portable(options) {
        Ok(()) => {
            crate::server::destroy::do_destroy(Vec::new(), options)?;
        }
        Err(e) if e.is::<InstanceNotFound>() => {
            crate::server::destroy::do_destroy(
                vec![("portable".into(), e)], options)?;
        }
        Err(e) => return Err(e),
    }
    Ok(())
}
