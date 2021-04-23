use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use dirs::home_dir;
use edgedb_client as client;
use serde::Serialize;

use crate::credentials::{self, get_connector};
use crate::platform::{get_current_uid};
use crate::process;
use crate::server::control::read_metadata;
use crate::server::detect::Lazy;
use crate::server::distribution::{MajorVersion};
use crate::server::docker::DockerCandidate;
use crate::server::errors::InstanceNotFound;
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::options::{StartConf, Start, Stop, Restart, Logs, Destroy};
use crate::server::os_trait::{CurrentOs, Method, Instance, InstanceRef};
use crate::server::package::PackageCandidate;
use crate::server::status::{Service, Status};
use crate::server::version::Version;
use crate::server::unix;
use crate::server::{debian, ubuntu, centos};


#[derive(Debug)]
pub struct Unknown {
    distro_name: String,
    distro_version: String,
    error: anyhow::Error,
}


#[derive(Debug)]
pub struct LocalInstance<'a> {
    method: &'a dyn Method,
    pub name: String,
    pub path: PathBuf,
    metadata: Lazy<Metadata>,
    slot: Lazy<String>,
    current_version: Lazy<Version<String>>,
}

impl LocalInstance<'_> {
    fn get_meta(&self) -> anyhow::Result<&Metadata> {
        self.metadata.get_or_try_init(|| read_metadata(&self.path))
    }
    fn get_slot(&self) -> anyhow::Result<&String> {
        self.slot.get_or_try_init(|| {
            match &self.get_meta()?.slot {
                Some(s) => Ok(s.clone()),
                None => anyhow::bail!("missing `slot` in metadata"),
            }
        })
    }
    fn socket_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(dirs::runtime_dir()
            .unwrap_or_else(|| {
                Path::new("/run/user").join(get_current_uid().to_string())
            })
            .join(format!("edgedb-{}", self.name)))
    }
}

impl Instance for LocalInstance<'_> {
    fn get_version(&self) -> anyhow::Result<&MajorVersion> {
        Ok(&self.get_meta()?.version)
    }
    fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>> {
        let meta = self.get_meta()?;
        if meta.version.is_nightly() {
            Ok(self.get_meta()?.current_version.as_ref())
        } else {
            self.current_version.get_or_try_init(|| {
                Ok(self.method.get_version(&meta.version.to_query())?
                    .version().clone())
            }).map(Some)
        }
    }
    fn get_port(&self) -> anyhow::Result<u16> {
        Ok(self.get_meta()?.port)
    }
    fn get_start_conf(&self) -> anyhow::Result<StartConf> {
        Ok(self.get_meta()?.start_conf)
    }
    fn start(&self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            process::run(&mut self.get_command()?)?;
        } else {
            process::run(Command::new("systemctl")
                .arg("--user")
                .arg("start")
                .arg(format!("edgedb-server@{}", self.name)))?;
        }
        Ok(())
    }
    fn stop(&self, _options: &Stop) -> anyhow::Result<()> {
        process::run(Command::new("systemctl")
            .arg("--user")
            .arg("stop")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn restart(&self, _options: &Restart) -> anyhow::Result<()> {
        process::run(Command::new("systemctl")
            .arg("--user")
            .arg("restart")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        if admin {
            let socket = self.socket_dir()?
                .join(format!(".s.EDGEDB{}.{}",
                    if admin { ".admin" } else { "" },
                    self.get_meta()?.port));
            let mut conn_params = client::Builder::new();
            conn_params.user("edgedb");
            conn_params.database("edgedb");
            conn_params.unix_addr(socket);
            Ok(conn_params)
        } else {
            get_connector(self.name())
        }
    }
    fn service_status(&self) -> anyhow::Result<()> {
        process::exit_from(Command::new("systemctl")
            .arg("--user")
            .arg("status")
            .arg(format!("edgedb-server@{}", self.name)))?;
        Ok(())
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn method(&self) -> &dyn Method {
        self.method
    }
    fn get_status(&self) -> Status {
        let system = false;
        let service = systemd_status(&self.name, system);
        let service_exists = systemd_service_path(&self.name, system)
            .map(|p| p.exists())
            .unwrap_or(false);
        unix::status(&self.name, &self.path, service_exists, service)
    }
    fn get_command(&self) -> anyhow::Result<Command> {
        let socket_dir = self.socket_dir()?;
        let mut cmd = Command::new(get_server_path(Some(self.get_slot()?)));
        cmd.arg("--port").arg(self.get_meta()?.port.to_string());
        cmd.arg("--data-dir").arg(&self.path);
        cmd.arg("--runstate-dir").arg(&socket_dir);
        Ok(cmd)
    }
    fn upgrade<'x>(&'x self, meta: &Metadata)
        -> anyhow::Result<InstanceRef<'x>>
    {
        Ok(LocalInstance {
            method: self.method,
            name: self.name.clone(),
            path: self.path.clone(),
            slot: Lazy::eager(meta.slot.as_ref()
                .expect("linux packages always have a slot").clone()),
            current_version: Lazy::eager(meta.current_version.as_ref()
                .expect("current version is known during upgrade").clone()),
            metadata: Lazy::eager(meta.clone()),
        }.into_ref())
    }
    fn revert(&self, metadata: &Metadata)
        -> anyhow::Result<()>
    {
        unix::revert(self, metadata)
    }
    fn logs(&self, logs: &Logs) -> anyhow::Result<()> {
        let mut cmd = Command::new("journalctl");
        cmd.arg("--user-unit").arg(unit_name(&self.name));
        if let Some(n) = logs.tail  {
            cmd.arg(format!("--lines={}", n));
        }
        if logs.follow {
            cmd.arg("--follow");
        }
        process::run(&mut cmd)
    }
}




impl CurrentOs for Unknown {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        Ok(InstallationMethods {
            package: PackageCandidate {
                supported: false,
                distro_name: self.distro_name.clone(),
                distro_version: self.distro_version.clone(),
                distro_supported: false,
                version_supported: false,
            },
            docker: DockerCandidate::detect()?,
        })
    }
    fn detect_all(&self) -> serde_json::Value {
        #[derive(Serialize)]
        struct Wrapper<'a> {
            distro_name: &'a str,
            distro_version: &'a str,
            error: String,
        }
        serde_json::to_value(Wrapper {
                distro_name: &self.distro_name,
                distro_version: &self.distro_version,
                error: format!("{:#}", self.error)
        }).expect("can serialize")
    }
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use InstallMethod::*;
        match method {
            Package => anyhow::bail!("Package method is unsupported on {}",
                                     self.distro_name),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

pub fn detect_distro() -> Result<Box<dyn CurrentOs>, anyhow::Error> {
    if Path::new("/etc/os-release").exists() {
        let rel = os_release::OsRelease::new()?;
        match &rel.id[..] {
            "debian" => Ok(Box::new(debian::Debian::new(&rel)?)),
            "ubuntu" => Ok(Box::new(ubuntu::Ubuntu::new(&rel)?)),
            "centos" => Ok(Box::new(centos::Centos::new(&rel)?)),
            _ => Ok(Box::new(Unknown {
                distro_name: rel.id.clone(),
                distro_version: rel.version_codename.clone(),
                error: anyhow::anyhow!("Unsupported distribution {:?}", rel.id)
            })),
        }
    } else if Path::new("/etc/centos-release").exists() {
        let data = fs::read_to_string("/etc/centos-release")
            .context("Reading /etc/centos-release")?;
        if let Some(dpos) = data.find('.') {
            if data.starts_with("CentOS release ") {
                let release = data["CentOS release ".len()..dpos]
                    .parse()
                    .context("bad /etc/centos-release file")?;
                return Ok(Box::new(centos::Centos::from_release(
                    release,
                )?));
            }
        }
        anyhow::bail!("Bad /etc/centos-release file")
    } else {
        Ok(Box::new(Unknown {
            distro_name: "<unknown>".into(),
            distro_version: "<unknown>".into(),
            error: anyhow::anyhow!("Cannot detect linux distribution, \
            no known /etc/*-release file found"),
        }))
    }
}

pub fn get_server_path(slot: Option<&String>) -> PathBuf {
    if let Some(slot) = slot {
        Path::new("/usr/bin").join(format!("edgedb-server-{}", slot))
    } else {
        PathBuf::from("/usr/bin/edgedb-server")
    }
}

pub fn systemd_unit(name: &str, meta: &Metadata) -> anyhow::Result<String> {
    let system = false;
    let path = unix::storage_dir(name)?;
    Ok(format!(r###"
[Unit]
Description=EdgeDB Database Service, instance {instance_name:?}
Documentation=https://edgedb.com/
After=syslog.target
After=network.target

[Service]
Type=notify
{userinfo}

Environment=EDGEDATA={directory}
RuntimeDirectory=edgedb-{instance_name}

ExecStart={server_path} --data-dir=${{EDGEDATA}} --runstate-dir=%t/edgedb-{instance_name} --port={port}
ExecReload=/bin/kill -HUP ${{MAINPID}}
KillMode=mixed
TimeoutSec=0

[Install]
WantedBy=multi-user.target
    "###,
        instance_name=name,
        directory=path.display(),
        server_path=get_server_path(meta.slot.as_ref()).display(),
        port=meta.port,
        userinfo=if system {
            "User=edgedb\n\
             Group=edgedb"
        } else {
            ""
        },
    ))
}

fn unit_dir(system: bool) -> anyhow::Result<PathBuf> {
    if system {
        Ok(PathBuf::from("/etc/systemd/system"))
    } else {
        Ok(home_dir()
            .context("Cannot determine home directory")?
            .join(".config/systemd/user"))
    }
}

fn unit_name(name: &str) -> String {
    format!("edgedb-server@{}.service", name)
}

pub fn systemd_service_path(name: &str, system: bool)
    -> anyhow::Result<PathBuf>
{
    Ok(unit_dir(system)?.join(&unit_name(name)))
}

pub fn create_systemd_service(name: &str, meta: &Metadata)
    -> anyhow::Result<()>
{
    let unit_dir = unit_dir(false)?;
    fs::create_dir_all(&unit_dir)?;
    let unit_name = unit_name(name);
    let unit_path = unit_dir.join(&unit_name);
    fs::write(&unit_path, systemd_unit(name, meta)?)?;
    process::run(Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload"))?;
    if meta.start_conf == StartConf::Auto {
        process::run(Command::new("systemctl")
            .arg("--user")
            .arg("enable")
            .arg(&unit_name))?;
    }
    Ok(())
}

pub fn systemd_status(name: &str, system: bool) -> Service {
    use Service::*;

    let mut cmd = Command::new("systemctl");
    if !system {
        cmd.arg("--user");
    }
    cmd.arg("show");
    cmd.arg(format!("edgedb-server@{}", name));
    let txt = match process::get_text(&mut cmd) {
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

pub fn all_instances<'x>(method: &'x dyn Method)
    -> anyhow::Result<Vec<InstanceRef<'x>>>
{
    let mut instances = BTreeSet::new();
    let user_base = unix::base_data_dir()?;
    if user_base.exists() {
        unix::instances_from_data_dir(&user_base, false, &mut instances)?;
    }
    Ok(instances.into_iter()
        .map(|(name, _)| LocalInstance {
            method,
            path: user_base.join(&name),
            name,
            metadata: Lazy::lazy(),
            slot: Lazy::lazy(),
            current_version: Lazy::lazy(),
        }.into_ref())
        .collect())
}

pub fn get_instance<'x>(method: &'x dyn Method, name: &str)
    -> anyhow::Result<InstanceRef<'x>>
{
    let dir = unix::base_data_dir()?.join(name);
    if dir.exists() {
        Ok(LocalInstance {
            method,
            path: dir,
            name: name.to_owned(),
            metadata: Lazy::lazy(),
            slot: Lazy::lazy(),
            current_version: Lazy::lazy(),
        }.into_ref())
    } else {
        Err(InstanceNotFound(
            anyhow::anyhow!("Directory '{}' does not exist", dir.display())
        ).into())
    }
}

fn systemd_is_not_found_error(e: &str) -> bool {
    e.contains("Failed to get D-Bus connection") ||
    e.contains("Failed to connect to bus") ||
    e.contains("No such file or directory") ||
    e.contains(".service not loaded") ||
    e.contains(".service does not exist")
}

pub fn destroy(options: &Destroy) -> anyhow::Result<()> {
    let system = false;
    let mut found = false;
    let name = &options.name;
    let svc_name = format!("edgedb-server@{}", name);
    log::info!(target: "edgedb::server::destroy",
        "Stopping service {}", svc_name);
    let mut not_found_error = None;
    let mut cmd = Command::new("systemctl");
    cmd.arg("--user");
    cmd.arg("stop");
    cmd.arg(&svc_name);
    match process::run_or_stderr(&mut cmd)? {
        Ok(()) => found = true,
        Err(e) if systemd_is_not_found_error(&e) => {
            not_found_error = Some(e);
        }
        Err(e) => Err(anyhow::anyhow!("Error running {:?}: {}", cmd, e))?,
    }

    let mut cmd = Command::new("systemctl");
    cmd.arg("--user");
    cmd.arg("disable");
    cmd.arg(&svc_name);
    match process::run_or_stderr(&mut cmd)? {
        Ok(()) => found = true,
        Err(e) if systemd_is_not_found_error(&e) => {
            not_found_error = Some(e);
        }
        Err(e) => Err(anyhow::anyhow!("Error running {:?}: {}", cmd, e))?,
    }

    let svc_path = systemd_service_path(name, system)?;
    if svc_path.exists() {
        found = true;
        fs::remove_file(&svc_path)?;
    }

    let dir = unix::storage_dir(name)?;
    if dir.exists() {
        found = true;
        log::info!(target: "edgedb::server::destroy",
            "Removing data directory {}", dir.display());
        fs::remove_dir_all(&dir)?;
    }
    let credentials = credentials::path(&options.name)?;
    if credentials.exists() {
        found = true;
        log::info!(target: "edgedb::server::destroy",
            "Removing credentials file {}", credentials.display());
        fs::remove_file(&credentials)?;
    }
    if found {
        Ok(())
    } else if let Some(e) = not_found_error {
        Err(InstanceNotFound(anyhow::anyhow!(
            "no instance {:?} found: {}", options.name, e.trim())).into())
    } else {
        Err(InstanceNotFound(anyhow::anyhow!(
            "no instance {:?} found", options.name)).into())
    }
}
