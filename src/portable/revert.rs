use fs_err as fs;

use crate::commands::ExitCode;
use crate::format;
use crate::question;
use crate::platform::tmp_file_path;
use crate::portable::control;
use crate::portable::create;
use crate::portable::exit_codes;
use crate::portable::local::Paths;
use crate::portable::status::{instance_status, DataDirectory, BackupStatus};
use crate::print::{self, eecho, Highlight};
use crate::process;
use crate::server::options::{Revert, StartConf};


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
    eecho!("EdgeDB version:", old_inst.installation.version);
    eecho!("Backup timestamp:",
        humantime::format_rfc3339(backup_info.timestamp),
        format!("({})", format::done_before(backup_info.timestamp)));
    if !options.ignore_pid_check {
        match status.data_status {
            DataDirectory::Upgrading(Ok(up)) if process::exists(up.pid) => {
                eecho!(
                    "Looks like upgrade is still in progress \
                    with pid", up.pid.emphasize(),
                );
                eecho!("Run with `--ignore-pid-check` to override");
                return Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
            }
            DataDirectory::Upgrading(_) => {
                eecho!("Note: it looks like backup is from a broken upgrade");
            }
            _ => {}
        }
    }
    if !options.no_confirm {
        eprintln!();
        eecho!("Currently stored data", "will be lost".emphasize(),
                  "and overwritten by the backup.");
        if old_inst.start_conf == StartConf::Manual {
            eecho!("Please ensure that server is stopped before proceeeding.");
        }
        let q = question::Confirm::new("Do you really want to revert?");
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

    let paths = Paths::get(&options.name)?;
    let tmp_path = tmp_file_path(&paths.data_dir);
    fs::rename(&paths.data_dir, &tmp_path)?;
    fs::rename(&paths.backup_dir, &paths.data_dir)?;

    let inst = old_inst;
    let mut exit = None;
    match (create::create_service(&inst), inst.start_conf) {
        (Ok(()), StartConf::Manual) => {
            eecho!("Instance", inst.name.emphasize(), "is reverted to",
                   inst.installation.version.emphasize());
            eecho!("You can start it manually via: \n  \
                edgedb instance start [--foreground] {}",
                inst.name);
        }
        (Ok(()), StartConf::Auto) => {
            control::do_restart(&inst)?;
            eecho!("Instance", inst.name.emphasize(),
                   "is successfully reverted to",
                   inst.installation.version.emphasize());
        }
        (Err(e), _) => {
            eecho!("Revert to", inst.installation.version.emphasize(),
                "is complete, \
                but there was an error creating the service:",
                format_args!("{:#}", e));
            eecho!("You can start it manually via:\n  \
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
