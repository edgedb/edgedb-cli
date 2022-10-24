use std::collections::BTreeSet;
use std::fmt::Display;
use std::fs;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Duration;

use anyhow::Context;
use async_std::future::TimeoutError;
use async_std::prelude::FutureExt;
use async_std::stream;
use async_std::task;
use fn_error_context::context;
use futures_util::{StreamExt, try_join};
use humantime::format_duration;

use edgedb_client::{Builder, credentials::Credentials};

use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::credentials;
use crate::format;
use crate::platform::{data_dir};
use crate::portable::control;
use crate::portable::exit_codes;
use crate::portable::local::{InstanceInfo, Paths};
use crate::portable::local::{read_ports, is_valid_instance_name, lock_file};
use crate::portable::options::{Status, List, instance_arg, InstanceName};
use crate::portable::upgrade::{UpgradeMeta, BackupMeta};
use crate::portable::{windows, linux, macos};
use crate::print::{self, echo, Highlight};
use crate::process;
use crate::table::{self, Table, Row, Cell};


#[derive(Debug)]
pub enum Service {
    Ready,
    Running { pid: u32 },
    Failed { exit_code: Option<u16> },
    Inactive { error: String },
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
    pub connection: Option<ConnectionStatus>,
    pub instance_status: Option<String>,
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
    pub instance_status: Option<String>,
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
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    let ref meta = InstanceInfo::read(name)?;
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
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => todo!(),
    };
    let meta = InstanceInfo::try_read(name).transpose();
    if let Some(meta) = meta {
        let paths = Paths::get(name)?;
        let status = status_from_meta(name, &paths, meta);
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
    Ok(builder.build()?.connect().await?.get_version().await?.to_owned())
}

async fn try_connect(creds: &Credentials) -> (Option<String>, ConnectionStatus)
{
    use async_std::future::timeout;
    match timeout(Duration::from_secs(2), try_get_version(creds)).await {
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

async fn _remote_status(name: &str, quiet: bool)
    -> anyhow::Result<RemoteStatus>
{
    let cred_path = credentials::path(&name)?;
    if !cred_path.exists() {
        if !quiet {
            echo!(print::err_marker(),
                  "No instance", name.emphasize(), "found");
        }
        return Err(ExitCode::new(exit_codes::INSTANCE_NOT_FOUND).into());
    }
    let cred_data = async_std::fs::read(cred_path).await?;
    let credentials = serde_json::from_slice(&cred_data)?;
    let (version, connection) = try_connect(&credentials).await;
    return Ok(RemoteStatus {
        name: name.into(),
        type_: RemoteType::Remote,
        credentials,
        version,
        connection: Some(connection),
        instance_status: None,
    })
}

async fn intermediate_feedback<F, D>(future: F, text: impl FnOnce() -> D)
    -> F::Output
    where F: Future,
          D: Display,
{
    future.race(async {
        task::sleep(Duration::from_millis(300)).await;
        if atty::is(atty::Stream::Stderr) {
            eprintln!("{}", text());
        }
        async_std::future::pending().await
    }).await
}

pub fn remote_status(options: &Status) -> anyhow::Result<()> {
    let name = match instance_arg(&options.name, &options.instance)? {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { .. } => unreachable!("remote_status got cloud instance")
    };

    let status = task::block_on(
        intermediate_feedback(
            _remote_status(name, options.quiet),
            || "Trying to connect...",
        )
    )?;
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
    } else if let Some(ConnectionStatus::Error(e)) = &status.connection {
        print::error(e);
    } else if let Some(conn_status) = &status.connection {
        println!("{}", conn_status.as_str());
    } else if let Some(inst_status) = &status.instance_status {
        println!("{}", inst_status);
    } else {
        println!("unknown");
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
        let name_op = fname.to_str().and_then(|x| is_valid_instance_name(x).then(|| x));
        if let Some(name) = name_op {
            return Some(Ok((name.into(), entry.path())))
        } else {
            log::info!("Skipping directory {:?}", entry.path());
            return None
        }
    }))
}

async fn get_remote_async(instances: Vec<String>) -> anyhow::Result<Vec<RemoteStatus>> {
    let mut result: Vec<_> = stream::from_iter(instances)
        .map(|name| async move {
            _remote_status(&name, false)
                .await
                .map_err(|e| log::warn!("Cannot check remote instance {:?}: {:#}", name, e))
                .ok()
        })
        .buffer_unordered(100)
        .flat_map(|x| stream::from_iter(x))
        .collect().await;
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

async fn get_cloud(cloud_client: &CloudClient) -> anyhow::Result<Vec<RemoteStatus>> {
    match crate::cloud::ops::list(cloud_client).await {
        Ok(rv) => Ok(rv),
        Err(e) if e.is::<TimeoutError>() => {
            print::warn("Timed out getting cloud instances.");
            Ok(Vec::new())
        }
        Err(e) => Err(e),
    }
}

async fn get_remote_and_cloud(
    instances: Vec<String>,
    cloud_client: &CloudClient,
) -> anyhow::Result<Vec<RemoteStatus>> {
    let (remote, cloud) = try_join!(
        get_remote_async(instances),
        get_cloud(cloud_client),
    )?;
    Ok(remote.into_iter().chain(cloud.into_iter()).collect())
}

pub fn get_remote(
    visited: &BTreeSet<String>,
    opts: &crate::options::Options,
) -> anyhow::Result<Vec<RemoteStatus>> {
    let cloud_client = CloudClient::new(&opts.cloud_options)?;
    let instances: Vec<_> = credentials::all_instance_names()?
        .into_iter()
        .filter(|name| !visited.contains(name))
        .collect();
    let num = instances.len();
    if cloud_client.is_logged_in {
        task::block_on(
            intermediate_feedback(
                get_remote_and_cloud(instances, &cloud_client),
                || {
                    if num > 0 {
                        format!("Checking cloud and {} remote instances...", num)
                    } else {
                        format!("Checking cloud instances...")
                    }
                },
            )
        )
    } else if num > 0 {
        task::block_on(
            intermediate_feedback(
                get_remote_async(instances),
                || format!("Checking {} remote instances...", num),
            )
        )
    } else {
        Ok(Vec::new())
    }
}

pub fn list(options: &List, opts: &crate::options::Options) -> anyhow::Result<()> {
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
        get_remote(&visited, opts)?
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
            Cell::new(if let Some(conn_status) = &status.connection {
                conn_status.as_str()
            } else {
                status.instance_status.as_deref().unwrap_or("unknown")
            }),
        ]));
    }
    table.printstd();
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
            Service::Ready => {
                println!("ready in socket activation mode, not running");
            }
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
            instance_status: None,
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
            Ready => {
                eprintln!("Ready in socket activation mode, not running");
            }
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
            Ready => exit(0),
            Running {..} => exit(0),
            Failed {..} => exit(3),
            Inactive {..} => exit(3),
        }
    }
}

impl RemoteStatus {
    pub fn print_extended(&self) {
        println!("{}:", self.name);
        let is_cloud = if let RemoteType::Cloud { instance_id } = &self.type_ {
            println!("  Cloud Instance ID: {}", instance_id);
            true
        } else {
            false
        };
        if let Some(conn_status) = &self.connection {
            println!("  Connection Status: {}", conn_status.as_str());
        }
        if let Some(inst_status) = &self.instance_status {
            println!("  Instance Status: {}", inst_status);
        }
        if !is_cloud {
            println!("  Credentials: exist");
        }
        println!("  Version: {}",
            self.version.as_ref().map_or("unknown", |x| &x[..]));
        let creds = &self.credentials;
        println!("  Host: {}",
            creds.host.as_ref().map_or("localhost", |x| &x[..]));
        println!("  Port: {}", creds.port);
        if !is_cloud {
            println!("  User: {}", creds.user);
            println!("  Database: {}",
                     creds.database.as_ref().map_or("edgedb", |x| &x[..]));
        }
        if let Some(ConnectionStatus::Error(e)) = &self.connection {
            println!("  Connection error: {:#}", e);
        }
    }

    pub fn json(&self) -> JsonStatus {
        JsonStatus {
            name: self.name.clone(),
            port: Some(self.credentials.port),
            version: self.version.clone(),
            service_status: None,
            remote_status: self.connection.as_ref().map(|s| s.as_str().to_string()),
            instance_status: self.instance_status.clone(),
            cloud_instance_id: if let RemoteType::Cloud { instance_id } = &self.type_ {
                Some(instance_id.clone())
            } else {
                None
            },
        }
    }

    pub fn exit(&self) -> ! {
        match &self.connection {
            Some(ConnectionStatus::Connected) => exit(0),
            Some(_) => exit(3),
            None => match &self.instance_status {
                Some(_) => exit(0),
                None => exit(4),
            }
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
        Service::Ready => "ready",
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
