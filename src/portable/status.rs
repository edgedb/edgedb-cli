use std::collections::BTreeSet;
use std::io;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::process::exit;

use anyhow::Context;
use async_std::task;
use async_std::net::TcpStream;
use async_std::io::timeout;
use fn_error_context::context;
use humantime::format_duration;

use edgedb_client::{Builder, credentials::Credentials};

use crate::commands::ExitCode;
use crate::credentials;
use crate::format;
use crate::platform::{data_dir};
use crate::portable::control;
use crate::portable::exit_codes;
use crate::portable::local::{InstanceInfo, Paths};
use crate::portable::local::{read_ports, is_valid_name, lock_file};
use crate::portable::options::{Status, List};
use crate::portable::upgrade::{UpgradeMeta, BackupMeta};
use crate::portable::{windows, linux, macos};
use crate::print::{self, echo, Highlight};
use crate::process;
use crate::table::{self, Table, Row, Cell};


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
    Exists {
        backup_meta: anyhow::Result<BackupMeta>,
        data_meta: anyhow::Result<InstanceInfo>,
    },
}

#[derive(Debug)]
pub struct FullStatus {
    pub name: String,
    pub service: Service,
    pub instance: anyhow::Result<InstanceInfo>,
    pub reserved_port: Option<u16>,
    pub port_status: Port,
    pub data_dir: PathBuf,
    pub data_status: DataDirectory,
    pub backup: BackupStatus,
    pub credentials_file_exists: bool,
    pub service_exists: bool,
    // TODO(tailhook) add linked projects
}

#[derive(Debug)]
pub enum ConnectionStatus {
    Connected,
    Refused,
    TimedOut,
    Error(anyhow::Error),
}

#[derive(Debug)]
pub enum RemoteType {
    Remote,
    Cloud {
        instance_id: String
    },
}

#[derive(Debug)]
pub struct RemoteStatus {
    pub name: String,
    pub type_: RemoteType,
    pub credentials: Credentials,
    pub version: Option<String>,
    pub connection: ConnectionStatus,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct JsonStatus {
    pub name: String,
    pub port: Option<u16>,
    pub version: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub service_status: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub remote_status: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub cloud_instance_id: Option<String>,
}


pub fn status(options: &Status) -> anyhow::Result<()> {
    if options.service {
        external_status(options)
    } else {
        normal_status(options)
    }
}

fn external_status(options: &Status) -> anyhow::Result<()> {
    let ref meta = InstanceInfo::read(&options.name)?;
    if cfg!(windows) {
        windows::external_status(meta)
    } else if cfg!(target_os="macos") {
        macos::external_status(meta)
    } else if cfg!(target_os="linux") {
        linux::external_status(meta)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

fn is_run_by_supervisor(name: &str) -> anyhow::Result<Option<bool>> {
    let lock_path = lock_file(name)?;
    match fs::read_to_string(&lock_path) {
        Ok(s) if s == "systemd" && cfg!(target_os="linux") => Ok(Some(true)),
        Ok(s) if s == "launchctl" && cfg!(target_os="macos") => Ok(Some(true)),
        Ok(_) => Ok(Some(false)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context(format!("cannot read {:?}", lock_path))?,
    }
}

fn service_status(name: &str) -> anyhow::Result<Service> {
    let run_by_super = is_run_by_supervisor(name)?;
    let mut pid = None;
    if !run_by_super.unwrap_or(false) {
        if let Some(file_pid) = control::read_pid(name)? {
            if process::exists(file_pid) {
                pid = Some(file_pid);
            }
        }
    };

    let service = if let Some(pid) = pid {
        Service::Running { pid }
    } else if control::detect_supervisor(name) {
        if cfg!(windows) {
            windows::service_status(name)
        } else if cfg!(target_os="macos") {
            macos::service_status(name)
        } else if cfg!(target_os="linux") {
            linux::service_status(name)
        } else {
            anyhow::bail!("unsupported platform")
        }
    } else {
        anyhow::bail!("no supervisor is found and no active pid exists");
    };
    Ok(service)
}

fn status_from_meta(name: &str, paths: &Paths,
                    instance: anyhow::Result<InstanceInfo>)
    -> FullStatus
{
    let service = service_status(name)
        .unwrap_or_else(|e| Service::Inactive { error: e.to_string() });
    let reserved_port = read_ports().ok()
        .and_then(|map| map.get(name).cloned());
    let port_status = probe_port(&instance, &reserved_port);
    let data_status = if paths.data_dir.exists() {
        if paths.upgrade_marker.exists() {
            DataDirectory::Upgrading(read_upgrade(&paths.upgrade_marker))
        } else {
            if instance.is_ok() {
                DataDirectory::Normal
            } else {
                DataDirectory::NoMetadata
            }
        }
    } else {
        DataDirectory::Absent
    };
    let backup = backup_status(name, &paths.backup_dir);
    let credentials_file_exists = paths.credentials.exists();
    let service_exists = paths.service_files.iter().any(|f| f.exists());
    return FullStatus {
        name: name.into(),
        service,
        instance,
        reserved_port,
        port_status,
        data_dir: paths.data_dir.clone(),
        data_status,
        backup,
        credentials_file_exists,
        service_exists,
    }
}

pub fn instance_status(name: &str) -> anyhow::Result<FullStatus> {
    let paths = Paths::get(name)?;   // the only error case
    let meta = InstanceInfo::read(&name);
    Ok(status_from_meta(name, &paths, meta))
}

fn normal_status(options: &Status) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name).transpose();
    if let Some(meta) = meta {
        let paths = Paths::get(&options.name)?;
        let status = status_from_meta(&options.name, &paths, meta);
        if options.debug {
            println!("{:#?}", status);
            Ok(())
        } else if options.extended {
            status.print_extended_and_exit();
        } else if options.json {
            status.print_json_and_exit();
        } else {
            status.print_and_exit();
        }
    } else {
        remote_status(options)
    }
}

async fn try_get_version(creds: &Credentials) -> anyhow::Result<String> {
    let mut builder = Builder::uninitialized();
    builder.credentials(creds)?;
    Ok(builder.build()?.connect().await?.get_version().await?)
}

fn try_connect(creds: &Credentials) -> (Option<String>, ConnectionStatus) {
    use async_std::future::timeout;

    let result = task::block_on(
        timeout(Duration::from_secs(2), try_get_version(creds))
    );
    match result {
        Ok(Ok(ver)) => (Some(ver), ConnectionStatus::Connected),
        Ok(Err(e)) => {
            let inner = e.source().and_then(|e| e.downcast_ref::<io::Error>());
            if let Some(e) = inner {
                if e.kind() == io::ErrorKind::ConnectionRefused {
                    return (None, ConnectionStatus::Refused);
                }
            }
            (None, ConnectionStatus::Error(e))
        }
        Err(_) => (None, ConnectionStatus::TimedOut)
    }
}

fn _remote_status(name: &str, quiet: bool) -> anyhow::Result<RemoteStatus> {
    let cred_path = credentials::path(&name)?;
    if !cred_path.exists() {
        if !quiet {
            echo!(print::err_marker(),
                  "No instance", name.emphasize(), "found");
        }
        return Err(ExitCode::new(exit_codes::INSTANCE_NOT_FOUND).into());
    }
    let file = io::BufReader::new(fs::File::open(cred_path)?);
    let credentials = serde_json::from_reader(file)?;
    let (version, connection) = try_connect(&credentials);
    return Ok(RemoteStatus {
        name: name.into(),
        type_: if let Some(instance_id) = &credentials.cloud_instance_id {
            RemoteType::Cloud { instance_id: instance_id.clone() }
        } else {
            RemoteType::Remote
        },
        credentials,
        version,
        connection,
    })
}

pub fn remote_status(options: &Status) -> anyhow::Result<()> {
    let status = _remote_status(&options.name, options.quiet)?;
    if options.service {
        println!("Remote instance");
    } else if options.debug {
        println!("{:#?}", status);
    } else if options.extended {
        status.print_extended();
    } else if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status.json())
                .expect("status is json-serializable"),
        );
    } else if let ConnectionStatus::Error(e) = &status.connection {
        print::error(e);
    } else {
        println!("{}", status.connection.as_str());
    }
    status.exit()
}

pub fn list_local<'x>(dir: &'x Path)
    -> anyhow::Result<
        impl Iterator<Item=anyhow::Result<(String, PathBuf)>> + 'x
    >
{
    let err_ctx = move || format!("error reading directory {:?}", dir);
    let dir = fs::read_dir(&dir).with_context(err_ctx)?;
    Ok(dir.filter_map(move |result| {
        let entry = match result {
            Ok(entry) => entry,
            res => return Some(Err(res.with_context(err_ctx).unwrap_err())),
        };
        let fname = entry.file_name();
        let name_op = fname.to_str().and_then(|x| is_valid_name(x).then(|| x));
        if let Some(name) = name_op {
            return Some(Ok((name.into(), entry.path())))
        } else {
            log::info!("Skipping directory {:?}", entry.path());
            return None
        }
    }))
}

pub fn get_remote(visited: &BTreeSet<String>)
    -> anyhow::Result<Vec<RemoteStatus>>
{
    let mut result = Vec::new();
    for name in credentials::all_instance_names()? {
        if visited.contains(&name) {
            continue;
        }
        match _remote_status(&name, false) {
            Ok(status) => result.push(status),
            Err(e) => {
                log::warn!(
                    "Cannot check remote instance {:?}: {:#}", name, e);
                continue;
            }
        }
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub fn list(options: &List, opts: &crate::options::Options) -> anyhow::Result<()> {
    if options.cloud {
        return task::block_on(crate::cloud::ops::list(options, opts));
    }

    let mut visited = BTreeSet::new();
    let mut local = Vec::new();
    let data_dir = data_dir()?;
    if data_dir.exists() {
        for pair in list_local(&data_dir)? {
            let (name, path) = pair?;
            if path.join("metadata.json").exists() {
                // consider deprecated instances remote,
                // i.e. not adding them in "visited"
                log::debug!("Instance {:?} has deprecated install method. \
                            Skipping.", name);
            } else {
                visited.insert(name.clone());
                local.push(instance_status(&name)?);
            }
        }
        local.sort_by(|a, b| a.name.cmp(&b.name));
    }
    let remote = if options.no_remote {
        Vec::new()
    } else {
        get_remote(&visited)?
    };

    if local.is_empty() && remote.is_empty() {
        if options.json {
            println!("[]");
        } else if !options.quiet {
            print::warn("No instances found");
        }
        return Ok(());
    }
    if options.debug {
        for status in local {
            println!("{:#?}", status);
        }
        for status in remote {
            println!("{:#?}", status);
        }
    } else if options.extended {
        for status in local {
            status.print_extended();
        }
        for status in remote {
            status.print_extended();
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(
            &local.iter().map(|status| status.json())
            .chain(remote.iter().map(|status| status.json()))
            .collect::<Vec<_>>()
        )?);
    } else {
        // using always JSON because we need that for windows impl
        let local_json = local.iter().map(|s| s.json()).collect::<Vec<_>>();
        print_table(&local_json, &remote);
    }

    Ok(())
}

pub fn print_table(local: &[JsonStatus], remote: &[RemoteStatus]) {
    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.set_titles(Row::new(
        ["Kind", "Name", "Port", "Version", "Status"]
        .iter().map(|x| table::header_cell(x)).collect()));
    for status in local {
        table.add_row(Row::new(vec![
            Cell::new("local"),
            Cell::new(&status.name),
            Cell::new(status.port.as_ref().map(ToString::to_string)
                .as_deref().unwrap_or("?")),
            Cell::new(status.version.as_deref().unwrap_or("?")),
            Cell::new(status.service_status.as_deref().unwrap_or("?")),
        ]));
    }
    for status in remote {
        table.add_row(Row::new(vec![
            Cell::new(match status.type_ {
                RemoteType::Cloud { instance_id: _ } => "cloud",
                RemoteType::Remote => "remote",
            }),
            Cell::new(&status.name),
            Cell::new(&format!("{}:{}",
                   status.credentials.host.as_deref().unwrap_or("localhost"),
                   status.credentials.port)),
            Cell::new(&status.version.as_ref()
                .map(|m| m.to_string()).as_deref().unwrap_or("?".into())),
            Cell::new(status.connection.as_str()),
        ]));
    }
    table.printstd();
}

pub fn probe_port(inst: &anyhow::Result<InstanceInfo>, reserved: &Option<u16>)
    -> Port
{
    use Port::*;

    let port = match inst.as_ref().ok().map(|m| m.port).or(*reserved) {
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

impl FullStatus {
    pub fn print_extended_and_exit(&self) -> ! {
        self.print_extended();
        self.exit()
    }
    fn print_extended(&self) {
        println!("{}:", self.name);

        print!("  Status: ");
        match &self.service {
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
            Service::Inactive { error } => {
                println!("inactive");
                println!("  Inactivity assumed because: {}", error);
            }
        }
        println!("  Service/Container: {}", match self.service_exists {
            true => "exists",
            false => "NOT FOUND",
        });
        println!("  Credentials: {}", match self.credentials_file_exists {
            true => "exists",
            false => "NOT FOUND",
        });

        match &self.instance {
            Ok(inst) => {
                if let Ok(version) = inst.get_version() {
                    println!("  Version: {}", version);
                }
                println!("  Startup: {}", inst.start_conf);
                if let Some(port) = self.reserved_port {
                    if inst.port == port {
                        println!("  Port: {}", port);
                    } else {
                        println!("  Port: {} (but {} reserved)",
                                 inst.port, port);
                    }
                } else {
                    println!("  Port: {}", inst.port);
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
            BackupStatus::Exists { backup_meta: Err(e), ..} => {
                format!("present (error: {:#})", e)
            }
            BackupStatus::Exists { backup_meta: Ok(b), .. } => {
                format!("present, {}", format::done_before(b.timestamp))
            }
        });
    }
    pub fn json(&self) -> JsonStatus {
        let meta = self.instance.as_ref().ok();
        JsonStatus {
            name: self.name.clone(),
            port: meta.map(|m| m.port),
            version: meta.and_then(|m| m.get_version().ok())
                .map(|v| v.to_string()),
            service_status: Some(status_str(&self.service).to_string()),
            remote_status: None,
            cloud_instance_id: None,
        }
    }
    pub fn print_json_and_exit<'x>(&'x self) -> ! {
        println!("{}",
            serde_json::to_string_pretty(&self.json())
            .expect("status is not json-serializable"));
        self.exit()
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

impl RemoteStatus {
    pub fn print_extended(&self) {
        println!("{}:", self.name);
        if let RemoteType::Cloud { instance_id } = &self.type_ {
            println!("  Cloud Instance ID: {}", instance_id);
        }
        println!("  Status: {}", self.connection.as_str());
        println!("  Credentials: exist");
        println!("  Version: {}",
            self.version.as_ref().map_or("unknown", |x| &x[..]));
        let creds = &self.credentials;
        println!("  Host: {}",
            creds.host.as_ref().map_or("localhost", |x| &x[..]));
        println!("  Port: {}", creds.port);
        println!("  User: {}", creds.user);
        println!("  Database: {}",
            creds.database.as_ref().map_or("edgedb", |x| &x[..]));
        if let ConnectionStatus::Error(e) = &self.connection {
            println!("  Connection error: {:#}", e);
        }
    }

    pub fn json(&self) -> JsonStatus {
        JsonStatus {
            name: self.name.clone(),
            port: Some(self.credentials.port),
            version: self.version.clone(),
            service_status: None,
            remote_status: Some(self.connection.as_str().to_string()),
            cloud_instance_id: if let RemoteType::Cloud { instance_id } = &self.type_ {
                Some(instance_id.clone())
            } else {
                None
            },
        }
    }

    pub fn exit(&self) -> ! {
        if matches!(self.connection, ConnectionStatus::Connected) {
            exit(0)
        } else {
            exit(3)
        }
    }
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "up",
            ConnectionStatus::Refused => "refused",
            ConnectionStatus::TimedOut => "timed out",
            ConnectionStatus::Error(..) => "error",
        }
    }
}

fn status_str(status: &Service) -> &'static str {
    match status {
        Service::Running {..} => "running",
        Service::Failed {..} => "not running",
        Service::Inactive {..} => "inactive",
    }
}

pub fn backup_status(name: &str, dir: &Path) -> BackupStatus {
    use BackupStatus::*;
    if !dir.exists() {
        return Absent;
    }
    let bmeta_json = dir.join("backup.json");
    let backup_meta = fs::read(&bmeta_json)
        .with_context(|| format!("error reading {}", bmeta_json.display()))
        .and_then(|data| serde_json::from_slice(&data)
        .with_context(|| format!("error decoding {}", bmeta_json.display())));
    let dmeta_json = dir.join("instance_info.json");
    let data_meta = InstanceInfo::read_at(name, &dmeta_json);
    Exists { backup_meta, data_meta }
}

#[context("failed to read upgrade marker {:?}", file)]
pub fn read_upgrade(file: &Path) -> anyhow::Result<UpgradeMeta> {
    Ok(serde_json::from_slice(&fs::read(&file)?)?)
}
