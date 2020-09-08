use std::io;
use std::fs;
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, exit};
use std::time::Duration;
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_std::task;
use async_std::net::TcpStream;
use async_std::io::timeout;
use once_cell::unsync::OnceCell;
use fn_error_context::context;
use prettytable::{Table, Row, Cell};

use crate::server::init::{read_ports};
use crate::server::upgrade::{UpgradeMeta, BackupMeta};
use crate::server::control::read_metadata;
use crate::server::metadata::Metadata;
use crate::server::{linux, macos};
use crate::server::is_valid_name;
use crate::process::get_text;
use crate::table;


#[derive(Debug)]
pub enum Service {
    Running { pid: u32 },
    Failed { exit_code: Option<u16> },
    Inactive { error: String },
}

#[derive(Debug)]
pub enum Port {
    Occupied,
    Refused,
    Unknown,
}

#[derive(Debug)]
pub enum DataDirectory {
    Absent,
    NoMetadata,
    Upgrading(anyhow::Result<UpgradeMeta>),
    Normal,
}

#[derive(Debug)]
pub enum BackupStatus {
    Absent,
    Exists(anyhow::Result<BackupMeta>),
}

#[derive(Debug)]
pub struct Status {
    name: String,
    service: Service,
    metadata: anyhow::Result<Metadata>,
    reserved_port: Option<u16>,
    port_status: Port,
    data_dir: PathBuf,
    data_status: DataDirectory,
    backup: BackupStatus,
    service_file_exists: bool,
}

pub struct Cache {
    launchctl_list: OnceCell<anyhow::Result<String>>,
    reserved_ports: OnceCell<Result<BTreeMap<String, u16>, ()>>,
}

fn format_duration(mut dur: Duration) -> String {
    if dur > Duration::from_secs(86400*2) {
        dur = Duration::from_secs((dur.as_secs() / 86400)*86400)
    } else {
        dur = Duration::from_secs(dur.as_secs());
    }
    humantime::format_duration(dur).to_string()
}

impl Status {
    pub fn print_extended_and_exit(&self) -> ! {
        self.print_extended();
        self.exit()
    }
    fn print_extended(&self) {
        println!("{}:", self.name);

        print!("  Status: ");
        match self.service {
            Service::Running { pid } => {
                println!("running, pid {}", pid);
                println!("  Pid: {}", pid);
            }
            Service::Failed { exit_code: Some(code) } => {
                println!("stopped, exit code {}", code);
            }
            Service::Failed { exit_code: None } => {
                println!("not running");
            }
            Service::Inactive {..} => {
                println!("inactive");
            }
        }
        println!("  Service file: {}", match self.service_file_exists {
            true => "exists",
            false => "NOT FOUND",
        });

        match &self.metadata {
            Ok(meta) => {
                println!("  Version: {}",meta.version.title());
                println!("  Installation method: {}", meta.method.title());
                println!("  Startup: {}", meta.start_conf);
                if let Some(port) = self.reserved_port {
                    if meta.port == port {
                        println!("  Port: {}", port);
                    } else {
                        println!("  Port: {} (but {} reserved)",
                                 meta.port, port);
                    }
                } else {
                    println!("  Port: {}", meta.port);
                }
            }
            _ => if let Some(port) = self.reserved_port {
                println!("  Port: {} (reserved)", port);
            },
        }

        println!("  Port status: {}", match &self.port_status {
            Port::Occupied => "occupied",
            Port::Refused => "unoccupied",
            Port::Unknown => "unknown",
        });

        println!("  Data directory: {}", self.data_dir.display());
        println!("  Data status: {}", match &self.data_status {
            DataDirectory::Absent => "NOT FOUND".into(),
            DataDirectory::NoMetadata => "METADATA ERROR".into(),
            DataDirectory::Upgrading(Err(e)) => format!("upgrading ({:#})", e),
            DataDirectory::Upgrading(Ok(up)) => {
                format!("upgrading {} -> {} for {}",
                        up.source, up.target,
                        format_duration(
                            up.started.elapsed().unwrap_or(Duration::new(0, 0))
                        ))
            }
            DataDirectory::Normal => "normal".into(),
        });
        println!("  Backup: {}", match &self.backup {
            BackupStatus::Absent => "absent".into(),
            BackupStatus::Exists(Err(e)) => {
                format!("present (error: {:#})", e)
            }
            BackupStatus::Exists(Ok(b)) => {
                format!("present, {}",
                    b.timestamp.elapsed()
                        .map(|d| format!("done {} ago", format_duration(d)))
                        .unwrap_or(format!("done just now")))
            }
        });
    }
    pub fn print_and_exit(&self) -> ! {
        use Service::*;
        match &self.service {
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
        self.exit()
    }
    fn exit(&self) -> ! {
        use Service::*;

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

fn launchctl_status(name: &str, _system: bool, cache: &Cache) -> Service {
    use Service::*;
    let list = cache.launchctl_list.get_or_init(|| {
        get_text(&mut Command::new("launchctl").arg("list"))
    });
    let txt = match list {
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

fn probe_port(metadata: &anyhow::Result<Metadata>, reserved: &Option<u16>)
    -> Port
{
    use Port::*;

    let port = match metadata.as_ref().ok().map(|m| m.port).or(*reserved) {
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

fn _get_status(base: &Path, name: &str, system: bool, cache: &Cache) -> Status
{
    use DataDirectory::*;

    let service = if cfg!(target_os="linux") {
        systemd_status(name, system)
    } else if cfg!(target_os="macos") {
        launchctl_status(name, system, &cache)
    } else {
        Service::Inactive { error: "unsupported os".into() }
    };
    let data_dir = base.join(name);
    let (data_status, metadata) = if data_dir.exists() {
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
    let reserved_port =
        cache.reserved_ports.get_or_init(|| {
            read_ports()
            .map_err(|e| log::warn!("{:#}", e))
        }).as_ref()
        .ok()
        .and_then(|ports| ports.get(name).cloned());
    let port_status = probe_port(&metadata, &reserved_port);
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
        reserved_port,
        port_status,
        data_dir,
        data_status,
        backup,
        service_file_exists,
    }
}

pub fn get_status(name: &str, system: bool) -> anyhow::Result<Status> {
    todo!();
    /*
    let base = data_path(system)?;
    let cache = Cache::new();
    Ok(_get_status(&base, name, system, &cache))
    */
}

fn get_status_with(name: &str, system: bool, cache: &Cache)
    -> anyhow::Result<Status>
{
    todo!();
    /*
    let base = data_path(system)?;
    Ok(_get_status(&base, name, system, cache))
    */
}

#[context("error reading dir {}", dir.display())]
fn instances_from_data_dir(dir: &Path, system: bool,
    instances: &mut BTreeSet<(String, bool)>)
    -> anyhow::Result<()>
{
    for item in fs::read_dir(&dir)? {
        let item = item?;
        if !item.file_type()?.is_dir() {
            continue;
        }
        if let Some(name) = item.file_name().to_str() {
            if !is_valid_name(name) {
                continue;
            }
            instances.insert((name.to_owned(), system));
        }
    }
    Ok(())
}

fn all_instances() -> anyhow::Result<BTreeSet<(String, bool)>> {
    /*
    let mut instances = BTreeSet::new();
    let user_base = data_path(false)?;
    if user_base.exists() {
        instances_from_data_dir(&user_base, false, &mut instances)?;
    }
    // TODO(tailhook) add a list of instances from the service directory
    // TODO(tailhook) add system instances
    Ok(instances)
    */
    todo!();
}

impl Cache {
    fn new() -> Cache {
        Cache {
            launchctl_list: OnceCell::new(),
            reserved_ports: OnceCell::new(),
        }
    }
}

pub fn print_status_all(extended: bool) -> anyhow::Result<()> {
    let instances = all_instances()?;
    let cache = Cache::new();
    let mut statuses = Vec::new();
    for (name, system) in instances {
        statuses.push(get_status_with(&name, system, &cache)?);
    }
    if statuses.is_empty() {
        eprintln!("No instances found");
        return Ok(());
    }
    if extended {
        for status in statuses {
            status.print_extended();
        }
    } else {
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Name", "Port", "Version", "Status"]
            .iter().map(|x| table::header_cell(x)).collect()));
        for status in statuses {
            table.add_row(Row::new(vec![
                Cell::new(&status.name),
                Cell::new(&status.metadata.as_ref()
                    .map(|m| m.port.to_string()).unwrap_or("?".into())),
                Cell::new(&status.metadata.as_ref()
                    .map(|m| m.version.title()).unwrap_or("?".into())),
                Cell::new(match status.service {
                    Service::Running {..} => "running",
                    Service::Failed {..} => "not running",
                    Service::Inactive {..} => "inactive",
                }),
            ]));
        }
        table.printstd();
    }
    Ok(())
}
