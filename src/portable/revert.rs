use fs_err as fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::format;
use crate::platform::tmp_file_path;
use crate::portable::control;
use crate::portable::create;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::Paths;
use crate::portable::options::{Revert, StartConf};
use crate::portable::status::{instance_status, DataDirectory, BackupStatus};
use crate::print::{self, echo, Highlight};
use crate::process;
use crate::question;


pub fn revert(options: &Revert) -> anyhow::Result<()> {
    use BackupStatus::*;

    let status = instance_status(&options.name)?;
    let (backup_info, old_inst) = match status.backup {
        Absent => anyhow::bail!("cannot find backup directory to revert"),
        Exists { backup_meta: Err(e), ..}
        => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists { data_meta: Err(e), ..}
        => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists { backup_meta: Ok(b), data_meta: Ok(d) } => (b, d),
    };
    echo!("EdgeDB version:", old_inst.installation.version);
    echo!("Backup timestamp:",
        humantime::format_rfc3339(backup_info.timestamp),
        format!("({})", format::done_before(backup_info.timestamp)));
    if !options.ignore_pid_check {
        match status.data_status {
            DataDirectory::Upgrading(Ok(up)) if process::exists(up.pid) => {
                echo!(
                    "Looks like upgrade is still in progress \
                    with pid", up.pid.emphasize(),
                );
                echo!("Run with `--ignore-pid-check` to override");
                return Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
            }
            DataDirectory::Upgrading(_) => {
                echo!("Note: it looks like backup is from a broken upgrade");
            }
            _ => {}
        }
    }
    if !options.no_confirm {
        eprintln!();
        echo!("Currently stored data", "will be lost".emphasize(),
                  "and overwritten by the backup.");
        if old_inst.start_conf == StartConf::Manual {
            echo!("Please ensure that server is stopped before proceeeding.");
        }
        let q = question::Confirm::new_dangerous(
            "Do you really want to revert?");
        if !q.ask()? {
            print::error("Canceled.");
            return Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
        }
    }

    if old_inst.start_conf != StartConf::Manual {
        if let Err(e) = control::do_stop(&options.name) {
            print::error(format!("Error stopping service: {:#}", e));
            if !options.no_confirm {
                let q = question::Confirm::new("Do you want to proceed?");
                if !q.ask()? {
                    print::error("Canceled.");
                    return Err(ExitCode::new(exit_codes::NOT_CONFIRMED))?;
                }
            }
        }
    }

    install::specific(&old_inst.installation.version.specific())
        .context("error installing old EdgeDB version")?;

    let paths = Paths::get(&options.name)?;
    let tmp_path = tmp_file_path(&paths.data_dir);
    fs::rename(&paths.data_dir, &tmp_path)?;
    fs::rename(&paths.backup_dir, &paths.data_dir)?;

    let inst = old_inst;
    let mut exit = None;
    echo!("Starting EdgeDB", inst.installation.version, "...");
    match (create::create_service(&inst), inst.start_conf) {
        (Ok(()), StartConf::Manual) => {
            echo!("Instance", inst.name.emphasize(), "is reverted to",
                   inst.installation.version.emphasize());
            echo!("You can start it manually via: \n  \
                edgedb instance start [--foreground] {}",
                inst.name);
        }
        (Ok(()), StartConf::Auto) => {
            control::do_restart(&inst)?;
            echo!("Instance", inst.name.emphasize(),
                   "is successfully reverted to",
                   inst.installation.version.emphasize());
        }
        (Err(e), _) => {
            echo!("Revert to", inst.installation.version.emphasize(),
                "is complete, \
                but there was an error creating the service:",
                format_args!("{:#}", e));
            echo!("You can start it manually via:\n  \
                edgedb instance start --foreground", inst.name);
            exit = Some(ExitCode::new(exit_codes::CANNOT_CREATE_SERVICE));
        }
    }

    fs::remove_file(paths.data_dir.join("backup.json"))?;
    fs::remove_dir_all(&tmp_path)?;
    if let Some(err) = exit {
        Err(err.into())
    } else {
        Ok(())
    }
}
