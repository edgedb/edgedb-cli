use std::io;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::process::exit;

use async_std::task;
use async_std::net::TcpStream;
use async_std::io::timeout;

use edgedb_client::{Builder, credentials::Credentials};

use crate::credentials;
use crate::portable::control::fallback;
use crate::portable::local::{InstanceInfo, Paths};
use crate::portable::{windows, linux, macos};
use crate::print::{self, eecho, Highlight};
use crate::server::create::read_ports;
use crate::server::options::{InstanceCommand, Status, List};


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

/* TODO(tailhook)
#[derive(Debug)]
pub enum DataDirectory {
    Absent,
    NoMetadata,
    Upgrading(anyhow::Result<UpgradeMeta>),
    Normal,
}
*/

#[derive(Debug)]
pub struct FullStatus {
    pub name: String,
    pub service: Service,
    pub instance: anyhow::Result<InstanceInfo>,
    pub reserved_port: Option<u16>,
    pub port_status: Port,
    pub data_dir: Option<PathBuf>,
    // pub data_status: DataDirectory,
    // pub backup: BackupStatus,  // TODO(tailhook)
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
struct RemoteStatus {
    pub name: String,
    pub credentials: Credentials,
    pub version: Option<String>,
    pub connection: ConnectionStatus,
}

#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
pub struct JsonStatus<'a> {
    name: &'a str,
    port: Option<u16>,
    version: Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    service_status: Option<&'a str>,
    #[serde(skip_serializing_if="Option::is_none")]
    remote_status: Option<&'a str>,
}


pub fn status(options: &Status) -> anyhow::Result<()> {
    if options.service {
        external_status(options)
    } else {
        normal_status(options)
    }
}

fn external_status(options: &Status) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        if cfg!(windows) {
            windows::external_status(meta)
        } else if cfg!(target_os="macos") {
            macos::external_status(meta)
        } else if cfg!(target_os="linux") {
            linux::external_status(meta)
        } else {
            anyhow::bail!("unsupported platform");
        }
    } else {
        fallback(&options.name, &InstanceCommand::Status(options.clone()))
    }
}

fn normal_status(options: &Status) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    // TODO(tailhook) provide (some) status even if there is no metadata
    if let Some(meta) = meta {
        let service = if cfg!(windows) {
            windows::service_status(&meta)
        } else if cfg!(target_os="macos") {
            macos::service_status(&meta)
        } else if cfg!(target_os="linux") {
            linux::service_status(&meta)
        } else {
            anyhow::bail!("unsupported platform");
        };
        let instance = Ok(meta);
        let reserved_port = read_ports().ok()
            .and_then(|map| map.get(&options.name).cloned());
        let port_status = probe_port(&instance, &reserved_port);
        let paths = Paths::get(&options.name);
        let data_dir = instance.as_ref().ok()
            .and_then(|i| i.data_dir().ok())
            .or_else(|| paths.as_ref().map(|p| p.data_dir.clone()).ok());
        let credentials_file_exists = paths.as_ref()
            .map(|p| p.credentials.exists()).unwrap_or(false);
        let service_exists = paths.as_ref()
            .map(|p| {
                p.service_files.iter().any(|f| f.exists())
            }).unwrap_or(false);
        let status = FullStatus {
            name: options.name.clone(),
            service,
            instance,
            reserved_port,
            port_status,
            data_dir,
            // data_status // TODO(tailhook)
            // backup: // TODO(tailhook)
            credentials_file_exists,
            service_exists,
        };
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
        match fallback(&options.name, &InstanceCommand::Status(options.clone())) {
            Ok(()) => Ok(()),
            Err(e) if e.is::<crate::server::errors::InstanceNotFound>() => {
                remote_status(options)
            }
            Err(e) => Err(e),
        }
    }
}

async fn try_get_version(creds: &Credentials) -> anyhow::Result<String> {
    let mut builder = Builder::uninitialized();
    builder.credentials(creds)?;
    Ok(builder.connect().await?.get_version().await?)
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

fn remote_status(options: &Status) -> anyhow::Result<()> {
    let cred_path = credentials::path(&options.name)?;
    if !cred_path.exists() {
        anyhow::bail!("No instance {:?} found", options.name);
    }
    let file = io::BufReader::new(fs::File::open(cred_path)?);
    let credentials = serde_json::from_reader(file)?;
    let (version, connection) = try_connect(&credentials);
    let status = RemoteStatus {
        name: options.name.clone(),
        credentials,
        version,
        connection,
    };
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

pub fn list(options: &List) -> anyhow::Result<()> {
    if options.deprecated_install_methods {
        return crate::server::status::print_status_all(
            options.extended, options.debug, options.json);
    }
    todo!();
    eecho!("Only portable packages shown here, \
        use `--deprecated-install-methods` \
        to show docker and package installations.".fade());
    Ok(())
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

        match &self.instance {
            Ok(inst) => {
                println!("  Version: {}", inst.installation.version);
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

        if let Some(data_dir) = &self.data_dir {
            println!("  Data directory: {}", data_dir.display());
        } else {
            println!("  Data directory: <cannot be determined>");
        }
        /* // TODO(tailhook)
        println!("  Data status: {}", match &self.data_status {
            DataDirectory::Absent => "NOT FOUND".into(),
            DataDirectory::NoMetadata => "METADATA ERROR".into(),
            /* TODO(tailhook)
            DataDirectory::Upgrading(Err(e)) => format!("upgrading ({:#})", e),
            DataDirectory::Upgrading(Ok(up)) => {
                format!("upgrading {} -> {} for {}",
                        up.source, up.target,
                        format_duration(
                            up.started.elapsed().unwrap_or(Duration::new(0, 0))
                        ))
            }
            */
            DataDirectory::Normal => "normal".into(),
        });
        */
        /* // TODO(tailhook)
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
        }); */
    }
    pub fn json<'x>(&'x self) -> JsonStatus<'x> {
        let meta = self.instance.as_ref().ok();
        JsonStatus {
            name: &self.name,
            port: meta.map(|m| m.port),
            version: meta.map(|m| m.installation.version.to_string()),
            service_status: Some(status_str(&self.service)),
            remote_status: None,
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

    pub fn json<'x>(&'x self) -> JsonStatus<'x> {
        JsonStatus {
            name: &self.name,
            port: Some(self.credentials.port),
            version: self.version.clone(),
            service_status: None,
            remote_status: Some(self.connection.as_str()),
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
