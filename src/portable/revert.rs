use fs_err as fs;

use anyhow::Context;

use crate::branding::BRANDING;
use crate::commands::ExitCode;
use crate::format;
use crate::platform::tmp_file_path;
use crate::portable::control;
use crate::portable::create;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::Paths;
use crate::portable::options::{instance_arg, InstanceName, Revert};
use crate::portable::status::{instance_status, BackupStatus, DataDirectory};
use crate::print::{self, echo, Highlight};
use crate::process;
use crate::question;

pub fn revert(options: &Revert) -> anyhow::Result<()> {
    use BackupStatus::*;

    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => {
            if cfg!(windows) {
                return crate::portable::windows::revert(options, name);
            } else {
                name
            }
        }
        InstanceName::Cloud { .. } => {
            print::error("This operation is not supported on cloud instances yet.");
            return Err(ExitCode::new(1))?;
        }
    };
    let status = instance_status(name)?;
    let (backup_info, old_inst) = match status.backup {
        Absent => anyhow::bail!("cannot find backup directory to revert"),
        Exists {
            backup_meta: Err(e),
            ..
        } => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists {
            data_meta: Err(e), ..
        } => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists {
            backup_meta: Ok(b),
            data_meta: Ok(d),
        } => (b, d),
    };
    echo!(BRANDING, " version: ", old_inst.get_version()?);
    echo!(
        "Backup timestamp:",
        humantime::format_rfc3339(backup_info.timestamp),
        format!("({})", format::done_before(backup_info.timestamp))
    );
    if !options.ignore_pid_check {
        match status.data_status {
            DataDirectory::Upgrading(Ok(up)) if process::exists(up.pid) => {
                echo!(
                    "Upgrade appears to still be in progress \
                    with pid",
                    up.pid.emphasize(),
                );
                echo!("Run with `--ignore-pid-check` to override");
                Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
            }
            DataDirectory::Upgrading(_) => {
                echo!("Note: backup appears to be from a broken upgrade");
            }
            _ => {}
        }
    }
    if !options.no_confirm {
        eprintln!();
        echo!(
            "Currently stored data",
            "will be lost".emphasize(),
            "and overwritten by the backup."
        );
        let q = question::Confirm::new_dangerous("Do you really want to revert?");
        if !q.ask()? {
            print::error("Canceled.");
            Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
        }
    }

    if let Err(e) = control::do_stop(name) {
        print::error(format!("Error stopping service: {:#}", e));
        if !options.no_confirm {
            let q = question::Confirm::new("Do you want to proceed?");
            if !q.ask()? {
                print::error("Canceled.");
                Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
            }
        }
    }

    install::specific(&old_inst.get_version()?.specific())
        .context("error installing old {BRANDING} version")?;

    let paths = Paths::get(name)?;
    let tmp_path = tmp_file_path(&paths.data_dir);
    fs::rename(&paths.data_dir, &tmp_path)?;
    fs::rename(&paths.backup_dir, &paths.data_dir)?;

    let inst = old_inst;
    echo!("Starting ", BRANDING, " ", inst.get_version()?, "...");

    create::create_service(&inst)
        .map_err(|e| {
            log::warn!("Error running {BRANDING} as a service: {e:#}");
        })
        .ok();

    control::do_restart(&inst)?;
    echo!(
        "Instance",
        inst.name.emphasize(),
        "is successfully reverted to",
        inst.get_version()?.emphasize()
    );

    fs::remove_file(paths.data_dir.join("backup.json"))?;
    fs::remove_dir_all(&tmp_path)?;
    Ok(())
}
