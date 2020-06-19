use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use crate::platform::{Uid, get_current_uid};
use crate::process::run;
use crate::server::detect::Lazy;
use crate::server::docker::DockerCandidate;
use crate::server::init;
use crate::server::install::{operation, exit_codes, Operation};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::options::StartConf;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::{PackageCandidate};
use crate::server::version::Version;
use crate::server::{debian, ubuntu, centos};

use anyhow::Context;
use dirs::home_dir;
use serde::Serialize;


#[derive(Debug)]
pub struct Unknown {
    distro_name: String,
    distro_version: String,
    error: anyhow::Error,
}

#[derive(Debug, Serialize)]
pub struct Linux {
    user_id: Lazy<Uid>,
    sudo_path: Lazy<Option<PathBuf>>,
}


impl Linux {
    pub fn new() -> Linux {
        Linux {
            user_id: Lazy::lazy(),
            sudo_path: Lazy::lazy(),
        }
    }
    pub fn detect_all(&self) {
        self.get_user_id();
        self.get_sudo_path();
    }
    pub fn get_user_id(&self) -> Uid {
        *self.user_id.get_or_init(|| {
            get_current_uid()
        })
    }
    pub fn get_sudo_path(&self) -> Option<&PathBuf> {
        self.sudo_path.get_or_init(|| {
            which::which("sudo").ok()
        }).as_ref()
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

pub fn perform_install(operations: Vec<Operation>, linux: &Linux)
    -> anyhow::Result<()>
{
    let mut ctx = operation::Context::new();
    let has_privileged = operations.iter().any(|x| x.is_privileged());
    if has_privileged && linux.get_user_id() != 0 {
        println!("The following commands will be run with elevated \
            privileges using sudo:");
        for op in &operations {
            if op.is_privileged() {
                println!("    {}", op.format(true));
            }
        }
        println!("Depending on system settings sudo may now ask \
                  you for your password...");
        match linux.get_sudo_path() {
            Some(cmd) => ctx.set_elevation_cmd(cmd),
            None => {
                eprintln!("`sudo` command not found. \
                           Cannot elevate acquire needed for \
                           installation. Please run \
                           `edgedb server install` as root user.");
                exit(exit_codes::NO_SUDO);
            }
        }
    }
    for op in &operations {
        op.perform(&ctx)?;
    }
    Ok(())
}

pub fn get_server_path(major_version: &Version<String>) -> PathBuf {
    Path::new("/usr/bin")
    .join(format!("edgedb-server-{}", major_version))
}

pub fn systemd_unit(settings: &init::Settings, meth: &dyn Method)
    -> anyhow::Result<String>
{
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
KillSignal=SIGINT
TimeoutSec=0

[Install]
WantedBy=multi-user.target
    "###,
        instance_name=settings.name,
        directory=settings.directory.display(),
        server_path=meth.get_server_path(&settings.version)?.display(),
        port=settings.port,
        userinfo=if settings.system {
            "User=edgedb\n\
             Group=edgedb"
        } else {
            ""
        },
    ))
}

pub fn create_systemd_service(settings: &init::Settings, meth: &dyn Method)
    -> anyhow::Result<()>
{
    let unit_dir = if settings.system {
        PathBuf::from("/etc/systemd/system")
    } else {
        home_dir()
            .context("Cannot determine home directory")?
            .join(".config/systemd/user")
    };
    fs::create_dir_all(&unit_dir)?;
    let unit_name = format!("edgedb-server@{}.service", settings.name);
    let unit_path = unit_dir.join(&unit_name);
    fs::write(&unit_path, systemd_unit(&settings, meth)?)?;
    run(Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload"))?;
    if settings.start_conf == StartConf::Auto {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("enable")
            .arg(&unit_name))?;
    }
    Ok(())
}
