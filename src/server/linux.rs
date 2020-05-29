use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use crate::server::detect::{Lazy, VersionQuery, VersionResult};
use crate::server::install::{operation, exit_codes, Operation};
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::package::{RepositoryInfo, PackageInfo};
use crate::server::version::Version;
use crate::server::{debian, ubuntu, centos};

use anyhow::Context;
use serde::Serialize;


#[derive(Debug)]
pub struct Unknown {
    error: anyhow::Error,
}

#[derive(Debug, Serialize)]
pub struct Linux {
    user_id: Lazy<users::uid_t>,
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
    pub fn get_user_id(&self) -> users::uid_t {
        *self.user_id.get_or_init(|| {
            users::get_current_uid()
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
        todo!();
    }
    fn detect_all(&self) -> serde_json::Value {
        #[derive(Serialize)]
        struct Wrapper {
            error: String
        }
        serde_json::to_value(Wrapper { error: self.error.to_string() })
        .expect("can serialize")
    }
    fn make_method<'x>(&'x self, _method: &InstallMethod,
        _methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        todo!();
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
            error: anyhow::anyhow!("Cannot detect linux distribution, \
            no known /etc/*-release file found"),
        }))
    }
}
fn version_matches(package: &PackageInfo, version: &VersionQuery) -> bool {
    use VersionQuery::*;

    if package.slot.is_none() ||
        (package.basename != "edgedb" && package.basename != "edgedb-server")
    {
        return false;
    }
    match version {
        Nightly => true,
        Stable(None) => true,
        Stable(Some(v)) => package.slot.as_ref() == Some(v),
    }
}

pub fn find_version(haystack: &RepositoryInfo, ver: &VersionQuery)
    -> Result<VersionResult, anyhow::Error>
{
    let mut max_version = None::<&PackageInfo>;
    for package in &haystack.packages {
        if version_matches(package, ver) {
            if let Some(max) = max_version {
                if max.version < package.version {
                    max_version = Some(package);
                }
            } else {
                max_version = Some(package);
            }
        }
    }
    if let Some(target) = max_version {
        let major = target.slot.as_ref().unwrap().clone();
        Ok(VersionResult {
            package_name:
                if major.to_ref() >= Version("1-alpha3") {
                    "edgedb-server".into()
                } else {
                    "edgedb".into()
                },
            major_version: major,
            version: target.version.clone(),
            revision: target.revision.clone(),
        })
    } else {
        anyhow::bail!("Version {} not found", ver)
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
