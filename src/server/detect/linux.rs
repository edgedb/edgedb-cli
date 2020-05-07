use serde::Serialize;

use crate::server::detect::Lazy;

#[derive(Clone, Debug, Serialize)]
pub struct OsInfo {
    distribution: Lazy<Distribution>,
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
    codename: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UbuntuInfo {
    codename: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CentosInfo {
    release: u32,
}

impl OsInfo {
    pub fn new() -> OsInfo {
        OsInfo {
            distribution: Lazy::lazy(),
        }
    }
    pub fn detect_all(&self) {
        self.get_distribution();
    }
    pub fn get_distribution(&self) -> &Distribution {
        self.distribution.get_or_init(|| {
            detect_distro().unwrap_or_else(|e| {
                log::warn!("Can't detect linux distribution: {}", e);
                Distribution::Unknown
            })
        })
    }
}

fn detect_distro() -> Result<Distribution, anyhow::Error> {
    use Distribution::*;

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
                .map_err(|e| anyhow::anyhow!("Error parsing version {:?}: {}",
                    rel.version_id, e))?,
        }),
        _ => Unknown,
    };
    Ok(distro)
}
