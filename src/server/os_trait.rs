use std::fmt;
use std::any::type_name;
use std::path::PathBuf;
use std::cmp::Ordering;

use crate::server::install;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::version::Version;
use crate::server::init;


pub trait CurrentOs: fmt::Debug + Send + Sync + 'static {
    fn get_type_name(&self) -> &'static str {
        type_name::<Self>()
    }
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods>;
    fn detect_all(&self) -> serde_json::Value;
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>;
}

pub trait Method: fmt::Debug + Send + Sync {
    fn name(&self) -> InstallMethod;
    fn install(&self, settings: &install::Settings) -> anyhow::Result<()>;
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<PreciseVersion>>;
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>;
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]>;
    fn detect_all(&self) -> serde_json::Value;
    fn is_system_only(&self) -> bool {
        false
    }
    fn get_server_path(&self, major_version: &Version<String>)
        -> anyhow::Result<PathBuf>;
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>;
}

#[derive(PartialEq, PartialOrd, Ord, Eq, Debug, Clone)]
pub enum MajorVersion {
    Stable(Version<String>),
    Nightly,
}

#[derive(Debug, Clone)]
pub struct PreciseVersion {
    major: MajorVersion,
    version: Version<String>,
}

impl PartialOrd for PreciseVersion {
    fn partial_cmp(&self, other: &PreciseVersion) -> Option<Ordering> {
        self.version.partial_cmp(&other.version)
    }
}

impl PartialEq for PreciseVersion {
    fn eq(&self, other: &PreciseVersion) -> bool {
        self.version.eq(&other.version)
    }
}

impl PreciseVersion {
    pub fn from_pair(major: &str, revision: &str) -> PreciseVersion {
        let nightly = revision.contains(".dev");
        PreciseVersion {
            major: if nightly {
                MajorVersion::Nightly
            } else {
                MajorVersion::Stable(Version(major.into()))
            },
            version: Version(format!("{}-{}", major, revision)),
        }
    }
    pub fn nightly(full_version: &str) -> PreciseVersion {
        PreciseVersion {
            major: MajorVersion::Nightly,
            version: Version(full_version.into()),
        }
    }
    pub fn major(&self) -> &MajorVersion {
        &self.major
    }
    pub fn as_str(&self) -> &str {
        self.version.num()
    }
    pub fn as_ver(&self) -> &Version<String> {
        &self.version
    }
}

impl MajorVersion {
    pub fn option(&self) -> String {
        match self {
            MajorVersion::Stable(v) => format!("--version={}", v.num()),
            MajorVersion::Nightly => "--nightly".into(),
        }
    }
    pub fn title(&self) -> &str {
        match self {
            MajorVersion::Stable(v) => v.num(),
            MajorVersion::Nightly => "nightly",
        }
    }
}
