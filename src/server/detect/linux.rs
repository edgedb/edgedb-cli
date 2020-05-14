use std::path::{Path, PathBuf};
use std::fs;

use anyhow::Context;
use serde::Serialize;

use crate::server::detect::Lazy;

#[derive(Clone, Debug, Serialize)]
pub struct OsInfo {
    distribution: Lazy<Distribution>,
    user_id: Lazy<users::uid_t>,
    sudo_path: Lazy<Option<PathBuf>>,
}

#[derive(Clone, Debug, Serialize)]
pub enum Distribution {
    Debian(DebianInfo),
    Ubuntu(UbuntuInfo),
    Centos(CentosInfo),
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct DebianInfo {
    pub codename: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UbuntuInfo {
    pub codename: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CentosInfo {
    pub release: u32,
}

impl OsInfo {
    pub fn new() -> OsInfo {
        OsInfo {
            distribution: Lazy::lazy(),
            user_id: Lazy::lazy(),
            sudo_path: Lazy::lazy(),
        }
    }
    pub fn detect_all(&self) {
        self.get_distribution();
        self.get_user_id();
        self.get_sudo_path();
    }
    pub fn get_distribution(&self) -> &Distribution {
        self.distribution.get_or_init(|| {
            detect_distro().unwrap_or_else(|e| {
                log::warn!("Can't detect linux distribution: {}", e);
                Distribution::Unknown
            })
        })
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

fn detect_distro() -> Result<Distribution, anyhow::Error> {
    use Distribution::*;

    if Path::new("/etc/os-release").exists() {
        let rel = os_release::OsRelease::new()?;
        let distro = match &rel.id[..] {
            "debian" => Debian(DebianInfo {
                codename: rel.version_codename,
            }),
            "ubuntu" => Ubuntu(UbuntuInfo {
                codename: rel.version_codename,
            }),
            "centos" => Centos(CentosInfo {
                release: rel.version_id.parse()
                    .map_err(|e|
                        anyhow::anyhow!("Error parsing version {:?}: {}",
                        rel.version_id, e))?,
            }),
            _ => Unknown,
        };
        Ok(distro)
    } else if Path::new("/etc/centos-release").exists() {
        let data = fs::read_to_string("/etc/centos-release")
            .context("Reading /etc/centos-release")?;
        if let Some(dpos) = data.find('.') {
            if data.starts_with("CentOS release ") {
                let release = data["CentOS release ".len()..dpos]
                    .parse()
                    .context("bad /etc/centos-release file")?;
                return Ok(Centos(CentosInfo { release }));
            }
        }
        Err(anyhow::anyhow!("Bad /etc/centos-release file"))?
    } else {
        Err(anyhow::anyhow!("Cannot detect linux distribution, \
            no known /etc/*-release file found"))
    }
}
