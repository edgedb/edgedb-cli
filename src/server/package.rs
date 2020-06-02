use async_std::task;
use serde::{Serialize, Deserialize};

use crate::server::version::Version;
use crate::server::detect::{Lazy, InstalledPackage};
use crate::server::os_trait::CurrentOs;
use crate::server::remote;


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

impl Lazy<Option<RepositoryInfo>> {
    pub fn get_or_fetch<F>(&self, f: F)
        -> anyhow::Result<Option<&RepositoryInfo>>
        where F: FnOnce() -> String,
    {
        self.get_or_try_init(|| {
            match task::block_on(remote::get_json(&f(),
                "cannot fetch package version info"))
            {
                Ok(data) => Ok(Some(data)),
                Err(error) => {
                    for cause in error.chain() {
                        match cause.downcast_ref::<remote::HttpFailure>() {
                            Some(e) if e.is_404() => return Ok(None),
                            Some(_) => break,
                            _ => {}
                        }
                    }
                    Err(error)
                }
            }
        }).map(|opt| opt.as_ref())
    }
}
