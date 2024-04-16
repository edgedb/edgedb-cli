use std::path::{PathBuf};

use fs_err as fs;

use crate::commands::ExitCode;
use crate::options::Options;
use crate::portable::control;
use crate::portable::exit_codes;
use crate::portable::local;
use crate::portable::options::{Destroy, instance_arg, InstanceName};
use crate::portable::project;
use crate::portable::windows;
use crate::print::{self, echo, Highlight};
use crate::question;


#[derive(Debug, thiserror::Error)]
#[error("instance not found")]
pub struct InstanceNotFound(#[source] pub anyhow::Error);


pub fn print_warning(name: &str, project_dirs: &[PathBuf]) {
    project::print_instance_in_use_warning(name, project_dirs);
    eprintln!("If you really want to destroy the instance, run:");
    eprintln!("  edgedb instance destroy -I {:?} --force", name);
}

pub fn with_projects(name: &str, force: bool,
                     warn: impl FnOnce(&str, &[PathBuf]),
                     f: impl FnOnce() -> anyhow::Result<()>)
    -> anyhow::Result<()>
{
    let project_dirs = project::find_project_dirs_by_instance(name)?;
    if !force && !project_dirs.is_empty() {
        warn(name, &project_dirs);
        Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
    }
    f()?;
    for dir in project_dirs {
        match project::read_project_path(&dir) {
            Ok(path) => eprintln!("Unlinking {}", path.display()),
            Err(_) => eprintln!("Cleaning {}", dir.display()),
        };
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn destroy(options: &Destroy, opts: &Options) -> anyhow::Result<()> {
    let name = instance_arg(&options.name, &options.instance)?;
    let name_str = name.to_string();
    with_projects(&name_str, options.force, print_warning, || {
        if !options.force && !options.non_interactive {
            let q = question::Confirm::new_dangerous(
                format!("Do you really want to delete instance {:?}?", name_str)
            );
            if !q.ask()? {
                print::error("Canceled.");
                return Err(ExitCode::new(exit_codes::NOT_CONFIRMED).into());
            }
        }
        match do_destroy(options, opts, name) {
            Ok(()) => Ok(()),
            Err(e) if e.is::<InstanceNotFound>() => {
                print::error(e);
                Err(ExitCode::new(exit_codes::INSTANCE_NOT_FOUND).into())
            }
            Err(e) => Err(e),
        }
    })?;
    if !options.quiet {
        echo!("Instance", name_str.emphasize(), "is successfully deleted.");
    }
    Ok(())
}

fn destroy_local(name: &str) -> anyhow::Result<()> {
    let paths = local::Paths::get(name)?;
    log::debug!("Paths {:?}", paths);
    let mut found = false;
    let mut not_found_err = None;
    match control::stop_and_disable(name) {
        Ok(f) => found = f,
        Err(e) if e.is::<InstanceNotFound>() => {
            not_found_err = Some(e);
        }
        Err(e) => {
            log::warn!("Error unloading service: {:#}", e);
        }
    }
    if paths.runstate_dir.exists() {
        found = true;
        log::info!("Removing runstate directory {:?}", paths.runstate_dir);
        fs::remove_dir_all(&paths.runstate_dir)?;
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

fn do_destroy(
    options: &Destroy, opts: &Options, name: &InstanceName
) -> anyhow::Result<()> {
    match name {
        InstanceName::Local(name) => {
            if cfg!(windows) {
                windows::destroy(options, name)
            } else {
                destroy_local(name)
            }
        },
        InstanceName::Cloud { org_slug, name: inst_name } => {
            log::info!("Removing cloud instance {}", name);
            if let Err(e) = crate::cloud::ops::destroy_cloud_instance(
                inst_name, org_slug, &opts.cloud_options
            ) {
                let msg = format!("Could not destroy EdgeDB Cloud instance: {:#}", e);
                if options.force {
                    print::warn(msg);
                } else {
                    anyhow::bail!(msg);
                }
            }
            Ok(())
        }
    }
}

pub fn force_by_name(name: &InstanceName, options: &Options) -> anyhow::Result<()> {
    do_destroy(&Destroy {
        name: None,
        instance: Some(name.clone()),
        verbose: false,
        force: true,
        quiet: false,
        non_interactive: true,
        cloud_opts: options.cloud_options.clone(),
    }, options, name)
}
