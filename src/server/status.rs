use std::collections::HashSet;
use std::io;
use std::fs;
use std::process::exit;
use std::time::Duration;
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_std::future;
use async_std::task;
use async_std::net::TcpStream;
use async_std::io::timeout;
use edgedb_client::Builder;
use fn_error_context::context;
use futures::stream::{self, StreamExt};
use prettytable::{Table, Row, Cell};

use crate::credentials;
use crate::format;
use crate::server::create::Storage;
use crate::server::detect;
use crate::server::distribution::MajorVersion;
use crate::server::metadata::Metadata;
use crate::server::methods::InstallMethod;
use crate::server::upgrade::{UpgradeMeta, BackupMeta};
use crate::server::version::Version;
use crate::table;


const REMOTE_STATUS_TIMEOUT: Duration = Duration::from_secs(1);

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
        data_meta: anyhow::Result<Metadata>,
    },
    Error(anyhow::Error),
}

#[derive(Debug)]
pub struct Status {
    pub method: InstallMethod,
    pub name: String,
    pub service: Service,
    pub metadata: anyhow::Result<Metadata>,
    pub reserved_port: Option<u16>,
    pub port_status: Port,
    pub storage: Storage,
    pub data_status: DataDirectory,
    pub backup: BackupStatus,
    pub credentials_file_exists: bool,
    pub service_exists: bool,
}

#[derive(Debug)]
pub enum RemoteStatusService {
    Running,
    Error(String),
    Unknown(String),
}

impl RemoteStatusService {
    pub fn display(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Error(_) => "error",
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn get_error(&self) -> Option<&String> {
        match self {
            Self::Running => None,
            Self::Error(error) => Some(error),
            Self::Unknown(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub struct RemoteStatus {
    pub name: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub database: Option<String>,
    pub version: Option<String>,
    pub major_version: Option<MajorVersion>,
    pub status: RemoteStatusService,
}

#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
pub struct JsonStatus<'a> {
    name: &'a str,
    port: Option<u16>,
    major_version: Option<&'a MajorVersion>,
    status: &'a str,
    method: &'a str,
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
        println!("  Service/Container: {}", match self.service_exists {
            true => "exists",
            false => "NOT FOUND",
        });
        println!("  Credentials: {}", match self.credentials_file_exists {
            true => "exist",
            false => "NOT FOUND",
        });

        match &self.metadata {
            Ok(meta) => {
                println!("  Version: {}",meta.version.title());
                println!("  Installation method: {}", self.method.title());
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

        println!("  Data directory: {}", self.storage.display());
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
            BackupStatus::Error(_) => {
                format!("error")
            }
        });
    }
    pub fn json<'x>(&'x self) -> JsonStatus<'x> {
        let meta = self.metadata.as_ref().ok();
        JsonStatus {
            name: &self.name,
            port: meta.map(|m| m.port),
            major_version: meta.map(|m| &m.version),
            status: status_str(&self.service),
            method: self.method.short_name(),
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
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            host: None,
            port: None,
            user: None,
            database: None,
            version: None,
            major_version: None,
            status: RemoteStatusService::Unknown("uninitialized".into())
        }
    }

    pub async fn probe(mut self, path: PathBuf) -> Self {
        let builder = match Builder::read_credentials(&path).await {
            Ok(builder) => builder,
            Err(e) => {
                self.status = RemoteStatusService::Error(format!("{}", e));
                return self;
            }
        };
        self.user = Some(builder.get_user().into());
        self.database = Some(builder.get_database().into());
        if let Some((host, port)) = builder.get_addr().get_tcp_addr() {
            self.host = Some(host.clone());
            self.port = Some(*port);
        } else if let Some(addr) = builder.get_addr().get_unix_addr() {
            self.host = Some(addr.display().to_string());
        }
        match future::timeout(
            REMOTE_STATUS_TIMEOUT, async {
                Ok::<String, anyhow::Error>(
                    builder.connect().await?.get_version().await?
                )
            }
        ).await {
            Ok(Ok(version)) => {
                self.status = RemoteStatusService::Running;
                self.major_version = Some(if version.contains("+dev") {
                    MajorVersion::Nightly
                } else if let Some((major_version, _))
                = version.split_once("+")
                {
                    MajorVersion::Stable(Version(major_version.into()))
                } else {
                    MajorVersion::Stable(Version(version.clone()))
                });
                self.version = Some(version);
            }
            Ok(Err(e)) => {
                log::info!(
                    "Error retrieving version from remote instance {}: {}",
                    self.name, e
                );
                self.status = RemoteStatusService::Error(format!("{}", e));
            }
            Err(e) => {
                log::info!(
                    "Timed out retrieving version from remote instance {}: {}",
                    self.name, e
                );
                self.status = RemoteStatusService::Unknown(
                    "probe timed out".into()
                );
            }
        };
        self
    }

    pub fn print_extended(&self) {
        println!("{}:", self.name);
        println!("  Status: {}", self.status.display());
        if let Some(error) = self.status.get_error() {
            println!("  Error: {}", error);
        }
        println!("  Credentials: exist");
        println!(
            "  Version: {}",
            self.major_version.as_ref()
            .map(|v| v.title()).unwrap_or("unknown".into())
        );
        if let Some(version) = self.version.as_ref() {
            println!("  Server Version: {}", version);
        }
        println!("  Installation method: remote");
        println!("  Host: {}",
                 self.host.as_ref().unwrap_or(&"?".into()));
        println!("  Port: {}",
                 self.port.map(|port| port.to_string()).unwrap_or("?".into()));
        println!("  User: {}",
                 self.user.as_ref().unwrap_or(&"?".into()));
        println!("  Database: {}",
                 self.database.as_ref().unwrap_or(&"?".into()));
    }

    pub fn json<'x>(&'x self) -> JsonStatus<'x> {
        JsonStatus {
            name: &self.name,
            port: self.port,
            major_version: self.major_version.as_ref(),
            status: self.status.display(),
            method: "remote",
        }
    }

    pub fn exit(&self) -> ! {
        if let RemoteStatusService::Running = self.status {
            exit(0)
        } else {
            exit(3)
        }
    }
}


pub fn probe_port(metadata: &anyhow::Result<Metadata>, reserved: &Option<u16>)
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
pub fn read_upgrade(file: &Path) -> anyhow::Result<UpgradeMeta> {
    Ok(serde_json::from_slice(&fs::read(&file)?)?)
}

pub fn backup_status(dir: &Path) -> BackupStatus {
    use BackupStatus::*;
    if !dir.exists() {
        return Absent;
    }
    let bmeta_json = dir.join("backup.json");
    let backup_meta = fs::read(&bmeta_json)
        .with_context(|| format!("error reading {}", bmeta_json.display()))
        .and_then(|data| serde_json::from_slice(&data)
        .with_context(|| format!("error decoding {}", bmeta_json.display())));
    let dmeta_json = dir.join("metadata.json");
    let data_meta = fs::read(&dmeta_json)
        .with_context(|| format!("error reading {}", dmeta_json.display()))
        .and_then(|data| serde_json::from_slice(&data)
        .with_context(|| format!("error decoding {}", dmeta_json.display())));
    Exists { backup_meta, data_meta }
}

pub fn print_status_all(extended: bool, debug: bool, json: bool)
    -> anyhow::Result<()>
{
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let mut local_names = HashSet::new();
    let mut statuses = Vec::new();
    for meth in methods.values() {
        statuses.extend(
            meth.all_instances()?
            .into_iter()
            .map(|i| {
                local_names.insert(String::from(i.name()));
                i.get_status()
            })
        );
    }

    let mut futures = Vec::new();
    let creds_dir = credentials::base_dir()?;
    if let Ok(creds_dir) = fs::read_dir(&creds_dir) {
        for item in creds_dir {
            let item = item?;
            if let Ok(filename) = item.file_name().into_string() {
                if let Some(name) = filename.strip_suffix(".json") {
                    if !local_names.contains(name) {
                        futures.push(
                            RemoteStatus::new(name).probe(item.path())
                        );
                    }
                }
            }
        }
    }
    let remote_statuses = task::block_on(
        stream::iter(futures)
            .buffer_unordered(32)
            .collect::<Vec<_>>()
    );

    if statuses.is_empty() && remote_statuses.is_empty() {
        if json {
            println!("[]");
        } else {
            eprintln!("No instances found");
        }
        return Ok(());
    }
    if debug {
        for status in statuses {
            println!("{:#?}", status);
        }
        for status in remote_statuses {
            println!("{:#?}", status);
        }
    } else if extended {
        for status in statuses {
            status.print_extended();
        }
        for status in remote_statuses {
            status.print_extended();
        }
    } else if json {
        println!("{}", serde_json::to_string_pretty(&statuses
            .iter()
            .map(|status| status.json())
            .chain(remote_statuses
                .iter()
                .map(|status| status.json())
            )
            .collect::<Vec<_>>()
        )?);
    } else {
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Name", "Port", "Version", "Installation", "Status"]
            .iter().map(|x| table::header_cell(x)).collect()));
        for status in statuses {
            table.add_row(Row::new(vec![
                Cell::new(&status.name),
                Cell::new(&status.metadata.as_ref()
                    .map(|m| m.port.to_string()).unwrap_or("?".into())),
                Cell::new(&status.metadata.as_ref()
                    .map(|m| m.version.title()).unwrap_or("?".into())),
                Cell::new(status.method.short_name()),
                Cell::new(status_str(&status.service)),
            ]));
        }
        for status in remote_statuses {
            table.add_row(Row::new(vec![
                Cell::new(&status.name),
                Cell::new(&status.port.map(|port| port.to_string())
                    .unwrap_or("?".into())),
                Cell::new(&status.major_version.as_ref()
                    .map(|m|m.title()).unwrap_or("?".into())
                ),
                Cell::new("remote"),
                Cell::new(status.status.display()),
            ]));
        }
        table.printstd();
    }
    Ok(())
}

fn status_str(status: &Service) -> &'static str {
    match status {
        Service::Running {..} => "running",
        Service::Failed {..} => "not running",
        Service::Inactive {..} => "inactive",
    }
}
