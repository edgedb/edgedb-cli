use std::fmt;
use std::collections::HashMap;

use once_cell::sync::OnceCell;
use serde::Serialize;

use crate::server::version::Version;

pub mod linux;
pub mod windows;
pub mod macos;

#[derive(Clone, Debug, Default)]
pub(in crate::server::detect) struct Lazy<T>(once_cell::sync::OnceCell<T>);

#[derive(Clone, Debug, Serialize)]
pub struct Detect {
    pub os_info: OsInfo,
    available_methods: Lazy<Vec<InstallMethod>>,
    installed: HashMap<InstallMethod, Lazy<Vec<InstalledPackage>>>,
}

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

#[derive(Clone, Debug, Serialize)]
pub enum OsInfo {
    Linux(linux::OsInfo),
    Windows(windows::OsInfo),
    Macos(macos::OsInfo),
    Unknown,
}

#[derive(Debug, Clone, Serialize, Hash, PartialEq, Eq)]
pub enum InstallMethod {
    OsRepository,
}

impl Detect {
    pub fn current_os() -> Detect {
        use OsInfo::*;

        let mut installed = HashMap::new();

        // Preinitialize all the methods
        installed.insert(InstallMethod::OsRepository, Lazy::lazy());

        Detect {
            os_info: if cfg!(windows) {
                Windows(windows::OsInfo::new())
            } else if cfg!(macos) {
                Macos(macos::OsInfo::new())
            } else if cfg!(target_os="linux") {
                Linux(linux::OsInfo::new())
            } else {
                Unknown
            },
            available_methods: Lazy::lazy(),
            installed,
        }
    }
    pub fn detect_all(&self) {
        use OsInfo::*;
        match &self.os_info {
            Windows(w) => w.detect_all(),
            Macos(m) => m.detect_all(),
            Linux(l) => l.detect_all(),
            Unknown => {}
        }
    }
    pub fn get_available_methods(&self) -> &[InstallMethod] {
        use linux::Distribution::*;
        use InstallMethod::*;

        self.available_methods.get_or_init(|| {
            match &self.os_info {
                OsInfo::Windows(_) => vec![],
                OsInfo::Macos(_) => vec![],
                OsInfo::Linux(l) => match l.get_distribution() {
                    Debian(_) => vec![OsRepository],
                    Ubuntu(_) => vec![OsRepository],
                    Centos(_) => vec![OsRepository],
                    Unknown => vec![]
                },
                OsInfo::Unknown => vec![]
            }
        })
    }
    pub fn get_version(&self, ver: &VersionQuery)
        -> Result<VersionResult, anyhow::Error>
    {
        use linux::Distribution::*;

        match &self.os_info {
            OsInfo::Windows(_) => anyhow::bail!("Unsupported"),
            OsInfo::Macos(_) => anyhow::bail!("Unsupported"),
            OsInfo::Linux(l) => match l.get_distribution() {
                Debian(d) => d.get_version(ver),
                Ubuntu(d) => d.get_version(ver),
                Centos(d) => d.get_version(ver),
                Unknown => anyhow::bail!("Unsupported"),
            },
            OsInfo::Unknown => anyhow::bail!("Unsupported"),
        }
    }

    pub fn get_installed(&self, meth: &InstallMethod) -> &[InstalledPackage] {
        self.installed.get(meth).unwrap()
        .get_or_init(|| {
            match &self.os_info {
                OsInfo::Windows(_) => vec![],
                OsInfo::Macos(_) => vec![],
                OsInfo::Linux(lin) => todo!(),
                OsInfo::Unknown => vec![]
            }
        })
    }
}

impl<T: Serialize> Serialize for Lazy<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer
    {
        self.0.get().serialize(serializer)
    }
}

impl<T> Lazy<T> {
    fn lazy() -> Lazy<T> {
        Lazy(OnceCell::new())
    }
    fn get_or_init<F>(&self, f: F) -> &T
        where F: FnOnce() -> T
    {
        self.0.get_or_init(f)
    }
    fn get_or_try_init<F, E>(&self, f: F) -> Result<&T, E>
        where F: FnOnce() -> Result<T, E>
    {
        self.0.get_or_try_init(f)
    }
}

pub fn main(_arg: &crate::server::options::Detect)
    -> Result<(), anyhow::Error>
{
    let det = Detect::current_os();
    det.detect_all();
    serde_json::to_writer_pretty(std::io::stdout(), &det)?;
    Ok(())
}

impl InstallMethod {
    pub fn title(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            OsRepository => "Native System Repository",
        }
    }
    pub fn option(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            OsRepository => "--method=native",
        }
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
