use std::io;
use std::fs;
use std::process::{Command, exit};
use std::time::Duration;
use std::path::Path;

use anyhow::Context;
use async_std::task;
use async_std::net::TcpStream;
use async_std::io::timeout;
use fn_error_context::context;

use crate::server::init::{Metadata, read_ports, data_path};
use crate::server::upgrade::{UpgradeMeta, BackupMeta};
use crate::server::control::read_metadata;
use crate::server::{linux, macos};
use crate::process::get_text;


pub enum Service {
    Running { pid: u32 },
    Failed { exit_code: Option<u16> },
    Inactive { error: String },
}

pub enum Port {
    Occupied,
    Refused,
    Unknown,
}

pub enum DataDirectory {
    Absent,
    NoMetadata,
    Upgrading(anyhow::Result<UpgradeMeta>),
    Normal,
}

pub enum BackupStatus {
    Absent,
    Exists(anyhow::Result<BackupMeta>),
}

pub struct Status {
    name: String,
    service: Service,
    metadata: anyhow::Result<Metadata>,
    allocated_port: Option<u16>,
    port_status: Port,
    data_directory: DataDirectory,
    backup: BackupStatus,
    service_file_exists: bool,
}

impl Status {
    pub fn print_and_exit(&self) -> ! {
        use Service::*;
        match self.service {
            Running { pid } => {
                eprint!("Running, pid ");
                println!("{}", pid);
            }
            Failed { exit_code: Some(code) } => {
                eprintln!("Stopped, exit code {}", code);
            }
            Failed { exit_code: None } => {
                eprintln!("Not running");
            }
            Inactive {..} => {
                eprintln!("Inactive");
            }
        }
        // TODO(tailhook) print more information in case some error is found:
        // Socket is occupied, while not running
        // No service file or no data directory
        // ..etc.
        match self.service {
            Running {..} => exit(0),
            Failed {..} => exit(3),
            Inactive {..} => exit(3),
        }
    }
}

fn systemd_status(name: &str, system: bool) -> Service {
    use Service::*;

    let mut cmd = Command::new("systemctl");
    if !system {
        cmd.arg("--user");
    }
    cmd.arg("show");
    cmd.arg(format!("edgedb-server@{}", name));
    let txt = match get_text(&mut cmd) {
        Ok(txt) => txt,
        Err(e) => {
            return Service::Inactive {
                error: format!("cannot determine service status: {:#}", e),
            }
        }
    };
    let mut pid = None;
    let mut exit = None;
    let mut load_error = None;
    for line in txt.lines() {
        if let Some(pid_str) = line.strip_prefix("MainPID=") {
            pid = pid_str.trim().parse().ok();
        }
        if let Some(status_str) = line.strip_prefix("ExecMainStatus=") {
            exit = status_str.trim().parse().ok();
        }
        if let Some(err) = line.strip_prefix("LoadError=") {
            load_error = Some(err.trim().to_string());
        }
    }
    match pid {
        None | Some(0) => {
            if let Some(error) = load_error {
                Inactive { error }
            } else {
                Failed { exit_code: exit }
            }
        }
        Some(pid) => {
            Running { pid }
        }
    }
}

fn launchctl_status(name: &str, _system: bool) -> Service {
    use Service::*;
    let txt = match get_text(&mut Command::new("launchctl").arg("list")) {
        Ok(txt) => txt,
        Err(e) => {
            return Service::Inactive {
                error: format!("cannot determine service status: {:#}", e),
            }
        }
    };
    let svc_name = format!("edgedb-server-{}", name);
    for line in txt.lines() {
        let mut iter = line.split_whitespace();
        let pid = iter.next().unwrap_or("-");
        let exit_code = iter.next();
        let cur_name = iter.next();
        if let Some(cur_name) = cur_name {
            if cur_name == svc_name {
                if pid == "-" {
                    return Failed {
                        exit_code: exit_code.and_then(|v| v.parse().ok()),
                    };
                }
                match pid.parse() {
                    Ok(pid) => return Running { pid },
                    Err(e) => return Inactive {
                        error: format!("invalid pid {:?}: {}", pid, e),
                    },
                }
            }
        }
    }
    Inactive { error: format!("service {:?} not found", svc_name) }
}

fn probe_port(metadata: &anyhow::Result<Metadata>, allocated: &Option<u16>)
    -> Port
{
    use Port::*;

    let port = match metadata.as_ref().ok().map(|m| m.port).or(*allocated) {
        Some(port) => port,
        None => return Unknown,
    };
    let probe = task::block_on(
        timeout(Duration::from_secs(1),
                TcpStream::connect(&("127.0.0.1", port)))
    );
    match probe {
        Ok(_) => Occupied,
        Err(e) if e.kind() == io::ErrorKind::TimedOut => {
            // This probably means that server doesn't accept connections but
            // port is occupied. Unless system is too overloaded.
            Occupied
        }
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => Refused,
        Err(_) => Unknown, // TODO(tailhook) should we show the error?
    }
}

#[context("failed to read upgrade file {}", file.display())]
fn read_upgrade(file: &Path) -> anyhow::Result<UpgradeMeta> {
    Ok(serde_json::from_slice(&fs::read(&file)?)?)
}

fn backup_status(dir: &Path) -> BackupStatus {
    use BackupStatus::*;
    if !dir.exists() {
        return Absent;
    }
    let meta_json = dir.join("backup.json");
    let meta = fs::read(&meta_json)
        .with_context(|| format!("error reading {}", meta_json.display()))
        .and_then(|data| serde_json::from_slice(&data)
        .with_context(|| format!("erorr decoding {}", meta_json.display())));
    Exists(meta)
}

fn _get_status(base: &Path, name: &str, system: bool) -> Status {
    use DataDirectory::*;

    let service = if cfg!(target_os="linux") {
        systemd_status(name, system)
    } else if cfg!(target_os="macos") {
        launchctl_status(name, system)
    } else {
        Service::Inactive { error: "unsupported os".into() }
    };
    let data_dir = base.join(name);
    let (data_directory, metadata) = if data_dir.exists() {
        let metadata = read_metadata(&data_dir);
        if metadata.is_ok() {
            let upgrade_file = data_dir.join("UPGRADE_IN_PROGRESS");
            if upgrade_file.exists() {
                (Upgrading(read_upgrade(&upgrade_file)), metadata)
            } else {
                (Normal, metadata)
            }
        } else {
            (NoMetadata, metadata)
        }
    } else {
        (Absent, Err(anyhow::anyhow!("No data directory")))
    };
    let allocated_port = read_ports()
        .map_err(|e| log::warn!("{:#}", e))
        .ok()
        .and_then(|ports| ports.get(name).cloned());
    let port_status = probe_port(&metadata, &allocated_port);
    let backup = backup_status(&base.join(format!("{}.backup", name)));
    let service_file_exists = if cfg!(target_os="linux") {
        linux::systemd_service_path(&name, system)
        .map(|p| p.exists())
        .unwrap_or(false)
    } else if cfg!(target_os="macos") {
        macos::launchd_plist_path(&name, system)
        .map(|p| p.exists())
        .unwrap_or(false)
    } else {
        false
    };

    Status {
        name: name.into(),
        service,
        metadata,
        allocated_port,
        port_status,
        data_directory,
        backup,
        service_file_exists,
    }
}

pub fn get_status(name: &str, system: bool) -> anyhow::Result<Status> {
    let base = data_path(system)?;
    Ok(_get_status(&base, name, system))
}
