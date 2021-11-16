use std::path::{PathBuf};

use fs_err as fs;

use crate::commands::ExitCode;
use crate::portable::exit_codes;
use crate::portable::local;
use crate::portable::project::{self};
use crate::portable::{windows, linux, macos};
use crate::server::errors::InstanceNotFound;
use crate::server::options::Destroy;


pub fn print_warning(name: &str, project_dirs: &[PathBuf]) {
    project::print_instance_in_use_warning(name, project_dirs);
    eprintln!("If you really want to destroy the instance, run:");
    eprintln!("  edgedb instance destroy {:?} --force", name);
}

pub fn destroy(options: &Destroy) -> anyhow::Result<()> {
    let project_dirs = project::find_project_dirs(&options.name)?;
    if !options.force && !project_dirs.is_empty() {
        print_warning(&options.name, &project_dirs);
        return Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
    }
    do_destroy(options)?;
    for dir in project_dirs {
        match project::read_project_real_path(&dir) {
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
    log::debug!("Paths {:?}", paths);
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
    if paths.backup_dir.exists() {
        found = true;
        log::info!("Removing backup directory {:?}", paths.backup_dir);
        fs::remove_dir_all(&paths.backup_dir)?;
    }
    if paths.dump_path.exists() {
        found = true;
        log::info!("Removing dump {:?}", paths.dump_path);
        fs::remove_dir_all(&paths.dump_path)?;
    }
    if paths.upgrade_marker.exists() {
        found = true;
        log::info!("Removing upgrade marker {:?}", paths.upgrade_marker);
        fs::remove_file(&paths.upgrade_marker)?;
    }
    if found {
        Ok(())
    } else if let Some(e) = not_found_err {
        Err(e)
    } else {
        Err(InstanceNotFound(anyhow::anyhow!("instance not found")).into())
    }
}

pub fn force_by_name(name: &str) -> anyhow::Result<()> {
    do_destroy(&Destroy {
        name: name.to_string(),
        verbose: false,
        force: true,
    })
}

fn do_destroy(options: &Destroy) -> anyhow::Result<()> {
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
