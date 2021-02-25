use std::io::{self, BufRead};

use crate::commands::ExitCode;
use crate::format;
use crate::process;
use crate::server::options::Revert;
use crate::server::os_trait::InstanceRef;
use crate::server::status::{BackupStatus, DataDirectory};


pub fn revert(instance: InstanceRef, options: &Revert)
    -> anyhow::Result<()>
{
    use BackupStatus::*;
    let status = instance.get_status();
    let (backup_info, data_info) = match status.backup {
        Absent => anyhow::bail!("cannot find backup directory to revert"),
        Exists { backup_meta: Err(e), ..}
        => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists { data_meta: Err(e), ..}
        => anyhow::bail!("cannot read backup metadata: {}", e),
        Exists { backup_meta: Ok(b), data_meta: Ok(d) } => (b, d),
        Error(e) => anyhow::bail!("backup status error: {}", e),
    };
    if let Some(old_ver) = &data_info.current_version {
        println!("EdgeDB version: {}", old_ver);
    } else {
        println!("EdgeDB version: {}", data_info.version.title());
    }
    println!("Backup timestamp: {} ({})",
        humantime::format_rfc3339(backup_info.timestamp),
        format::done_before(backup_info.timestamp));
    if !options.ignore_pid_check {
        match status.data_status {
            DataDirectory::Upgrading(Ok(up)) if process::exists(up.pid) => {
                eprintln!(
                    "edgedb error: Looks like upgrade is still in progress \
                    with pid {}", up.pid);
                eprintln!("Run with `--ignore-pid-check` to overrride");
                return Err(ExitCode::new(3))?;
            }
            DataDirectory::Upgrading(_) => {
                println!(
                    "Note: it looks like backup is from a broken upgrade");
            }
            _ => {}
        }
    }
    if !options.no_confirm {
        println!();
        println!("Currently stored data will be LOST \
                  and overwritten by the backup.");
        println!("Do you really want to revert? (type `Yes`)");
        if let Some(Ok(val)) = io::BufReader::new(io::stdin()).lines().next() {
            if val != "Yes" {
                eprintln!("Canceled ({:?} != \"Yes\")", val);
                return Err(ExitCode::new(2))?;
            }
        } else {
            eprintln!("Canceled");
            return Err(ExitCode::new(1))?;
        }
    }
    instance.revert(&data_info)?;
    Ok(())
}
