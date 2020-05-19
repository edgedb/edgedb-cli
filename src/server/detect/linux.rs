use std::path::{Path, PathBuf};
use std::fs;

use anyhow::Context;
use async_std::task;
use serde::{Serialize, Deserialize};

use crate::server::detect::{Lazy, VersionQuery, VersionResult};
use crate::server::remote;
use crate::server::version::Version;

#[derive(Clone, Debug, Serialize)]
pub struct OsInfo {
    distribution: Lazy<Distribution>,
    user_id: Lazy<users::uid_t>,
    sudo_path: Lazy<Option<PathBuf>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VersionInfo {
    packages: Vec<PackageInfo>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageInfo {
    basename: String,
    slot: Option<Version<String>>,
    version: Version<String>,
    revision: String,
    architecture: String,
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
    #[serde(skip)]
    packages: Lazy<VersionInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UbuntuInfo {
    pub codename: String,
    #[serde(skip)]
    packages: Lazy<VersionInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CentosInfo {
    pub release: u32,
    #[serde(skip)]
    packages: Lazy<VersionInfo>,
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
        use Distribution::*;

        self.get_distribution();
        self.get_user_id();
        self.get_sudo_path();
        match self.get_distribution() {
            Ubuntu(u) => u.detect_all(),
            Debian(d) => d.detect_all(),
            Centos(c) => c.detect_all(),
            Unknown => {}
        }
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
                packages: Lazy::lazy(),
            }),
            "ubuntu" => Ubuntu(UbuntuInfo {
                codename: rel.version_codename,
                packages: Lazy::lazy(),
            }),
            "centos" => Centos(CentosInfo {
                release: rel.version_id.parse()
                    .map_err(|e|
                        anyhow::anyhow!("Error parsing version {:?}: {}",
                        rel.version_id, e))?,
                packages: Lazy::lazy(),
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
                return Ok(Centos(CentosInfo {
                    release,
                    packages: Lazy::lazy(),
                }));
            }
        }
        Err(anyhow::anyhow!("Bad /etc/centos-release file"))?
    } else {
        Err(anyhow::anyhow!("Cannot detect linux distribution, \
            no known /etc/*-release file found"))
    }
}

impl UbuntuInfo {
    fn detect_all(&self) {
    }
    pub fn get_version(&self, ver: &VersionQuery)
        -> Result<VersionResult, anyhow::Error>
    {
        use VersionQuery::*;

        let packages = self.packages.get_or_try_init(|| {
            task::block_on(remote::get_json(
                &format!(
                    "https://packages.edgedb.com/apt/.jsonindexes/{}{}.json",
                    self.codename,
                    if matches!(ver, Nightly) { ".nightly" } else { "" }),
                "cannot fetch package version info"))
        })?;
        find_version(packages, ver)
    }
}

impl DebianInfo {
    fn detect_all(&self) {
    }
    pub fn get_version(&self, ver: &VersionQuery)
        -> Result<VersionResult, anyhow::Error>
    {
        use VersionQuery::*;

        let packages = self.packages.get_or_try_init(|| {
            task::block_on(remote::get_json(
                &format!(
                    "https://packages.edgedb.com/apt/.jsonindexes/{}{}.json",
                    self.codename,
                    if matches!(ver, Nightly) { ".nightly" } else { "" }),
                "cannot fetch package version info"))
        })?;
        find_version(packages, ver)
    }
}

impl CentosInfo {
    fn detect_all(&self) {
    }
    pub fn get_version(&self, ver: &VersionQuery)
        -> Result<VersionResult, anyhow::Error>
    {
        use VersionQuery::*;

        let packages = self.packages.get_or_try_init(|| {
            task::block_on(remote::get_json(
                &format!(
                    "https://packages.edgedb.com/rpm/.jsonindexes/el{}{}.json",
                    self.release,
                    if matches!(ver, Nightly) { ".nightly" } else { "" }),
                "cannot fetch package version info"))
        })?;
        find_version(packages, ver)
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

fn find_version(haystack: &VersionInfo, ver: &VersionQuery)
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

