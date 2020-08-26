use serde::{Serialize, Deserialize};

use crate::server::version::Version;
use crate::server::detect::{Lazy, InstalledPackage, VersionQuery};
use crate::server::detect::{VersionResult};
use crate::server::os_trait::{CurrentOs, PreciseVersion};


#[derive(Debug, Serialize)]
pub struct PackageCandidate {
    pub supported: bool,
    pub distro_name: String,
    pub distro_version: String,
    pub distro_supported: bool,
    pub version_supported: bool,
}

#[derive(Debug, Serialize)]
pub struct PackageMethod<'os, O: CurrentOs + ?Sized> {
    #[serde(skip)]
    pub os: &'os O,
    #[serde(skip)]
    pub installed: Lazy<Vec<InstalledPackage>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RepositoryInfo {
    pub packages: Vec<PackageInfo>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageInfo {
    pub basename: String,
    pub slot: Option<Version<String>>,
    pub version: Version<String>,
    pub revision: String,
    pub architecture: String,
}

impl PackageCandidate {
    pub fn format_option(&self, buf: &mut String, recommended: bool) {
        use std::fmt::Write;

        write!(buf, " * --method=package -- to install {} native package",
            self.distro_name).unwrap();
        if recommended {
            buf.push_str(" (recommended)");
        }
        buf.push('\n');
    }

    pub fn format_error(&self, buf: &mut String) {
        use std::fmt::Write;

        if self.distro_supported {
            write!(buf,
                " * Note: native packages are not supported for {} {}",
                self.distro_name,
                self.distro_version).unwrap();
        } else {
            buf.push_str(" * Note: native packages are \
                             not supported for this platform");
        }
        buf.push('\n');
    }
    pub fn make_method<'os, O>(&self, os: &'os O)
        -> anyhow::Result<PackageMethod<'os, O>>
        where O: CurrentOs + ?Sized,
    {
        if !self.supported {
            anyhow::bail!("Method `package` is not supported");
        }
        Ok(PackageMethod {
            os,
            installed: Lazy::lazy(),
        })
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
    let mut max_version = None::<(&PackageInfo, Version<String>)>;
    for package in &haystack.packages {
        if version_matches(package, ver) {
            let cur_version = package.full_version();
            if let Some((_, max_ver)) = &max_version {
                if max_ver < &cur_version {
                    max_version = Some((package, cur_version));
                }
            } else {
                max_version = Some((package, cur_version));
            }
        }
    }
    if let Some((target, _)) = max_version {
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

impl PackageInfo {
    pub fn is_nightly(&self) -> bool {
        return self.version.as_ref().contains(".dev")
    }
    pub fn precise_version(&self) -> PreciseVersion {
        let slot = self.slot.as_ref().expect("only server packages supported");
        if self.is_nightly() {
            PreciseVersion::nightly(&format!("{}-{}", slot, self.revision))
        } else {
            PreciseVersion::from_pair(slot.num(), &self.revision)
        }
    }
    pub fn full_version(&self) -> Version<String> {
        Version(format!("{}-{}", self.version, self.revision))
    }
}
