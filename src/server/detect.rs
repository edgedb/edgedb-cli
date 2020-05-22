use std::fmt;

use once_cell::sync::OnceCell;
use serde::Serialize;

use crate::server::version::Version;
use crate::server::docker::{DockerCandidate};
use crate::server::package::{PackageCandidate};
use crate::server::os_trait::CurrentOs;
use crate::server::install::InstallMethod;

use anyhow::Context;


#[cfg(target_arch="x86_64")]
pub const ARCH: &str = "x86_64";
#[cfg(not(any(
    target_arch="x86_64",
)))]
compile_error!("Unsupported architecture, supported: x86_64");

#[derive(Clone, Debug, Default)]
pub struct Lazy<T>(once_cell::sync::OnceCell<T>);

#[derive(Debug)]
pub enum VersionQuery {
    Stable(Option<Version<String>>),
    Nightly,
}

#[derive(Clone, Serialize, Debug)]
pub struct VersionResult {
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub revision: String,
}

#[derive(Clone, Serialize, Debug)]
pub struct InstalledPackage {
    pub method: InstallMethod,
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub revision: String,
}

#[derive(Debug)]
pub struct InstallationMethods {
    pub package: PackageCandidate,
    pub docker: DockerCandidate,
}

impl<T: Serialize> Serialize for Lazy<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer
    {
        self.0.get().serialize(serializer)
    }
}

impl<T> Lazy<T> {
    pub fn lazy() -> Lazy<T> {
        Lazy(OnceCell::new())
    }
    pub fn get_or_init<F>(&self, f: F) -> &T
        where F: FnOnce() -> T
    {
        self.0.get_or_init(f)
    }
    pub fn get_or_try_init<F, E>(&self, f: F) -> Result<&T, E>
        where F: FnOnce() -> Result<T, E>
    {
        self.0.get_or_try_init(f)
    }
}

pub fn current_os() -> anyhow::Result<Box<dyn CurrentOs>> {
    use crate::server::{windows, macos, linux, unknown_os};

    if cfg!(windows) {
        Ok(Box::new(windows::Windows::new()))
    } else if cfg!(macos) {
        Ok(Box::new(macos::Macos::new()))
    } else if cfg!(target_os="linux") {
        linux::detect_distro()
            .context("error detecting linux distribution")
    } else {
        Ok(Box::new(unknown_os::Unknown::new()))
    }
}

pub fn main(_arg: &crate::server::options::Detect)
    -> Result<(), anyhow::Error>
{
    let os = current_os()?;
    serde_json::to_writer_pretty(std::io::stdout(), &os.detect_all())?;
    Ok(())
}

impl VersionQuery {
    pub fn is_nightly(&self) -> bool {
        matches!(self, VersionQuery::Nightly)
    }
}

impl fmt::Display for VersionQuery {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use VersionQuery::*;
        match self {
            Stable(None) => "stable".fmt(f),
            Stable(Some(ver)) => ver.fmt(f),
            Nightly => "nightly".fmt(f),
        }
    }
}

impl InstallationMethods {
    pub fn format_error(&self) -> String {
        let mut buf = String::with_capacity(1024);
        if self.package.supported || self.docker.supported {
            buf.push_str("No installation method chosen, add:\n");
            if self.package.supported {
                self.package.format_option(&mut buf, true);
            }
            if self.docker.supported {
                self.docker.format_option(&mut buf, !self.package.supported);
            }
            if !self.package.supported {
                self.package.format_error(&mut buf);
            }
            if !self.docker.supported {
                self.docker.format_error(&mut buf);
            }
            buf.push_str("or run `edgedb server install --interactive` \
                          and follow instructions");
        } else if self.docker.platform_supported {
            buf.push_str("No installation method found:\n");
            self.package.format_error(&mut buf);
            self.docker.format_error(&mut buf);
            buf.push_str("Consider installing docker: \
                https://docs.docker.com/get-docker/");
            buf.push_str("Or ask for native support at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
        } else {
            buf.push_str("No installation method supported for the platform:");
            self.package.format_error(&mut buf);
            self.docker.format_error(&mut buf);
            buf.push_str("Please consider opening an issue at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
        }
        return buf;
    }
    pub fn pick_first(&self) -> Option<InstallMethod> {
        if self.package.supported {
            Some(InstallMethod::Package)
        } else if self.docker.supported {
            Some(InstallMethod::Docker)
        } else {
            None
        }
    }
}
