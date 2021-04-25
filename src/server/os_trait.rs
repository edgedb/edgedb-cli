use std::any::type_name;
use std::cmp::Ordering;
use std::fmt;
use std::process::Command;

use edgedb_client as client;

use crate::server::detect::VersionQuery;
use crate::server::distribution::{DistributionRef, MajorVersion};
use crate::server::init::{self, Storage};
use crate::server::install;
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallMethod, InstallationMethods};
use crate::server::options::{Destroy, StartConf, Upgrade};
use crate::server::options::{Logs, Restart, Start, Stop};
use crate::server::status::Status;
use crate::server::upgrade;
use crate::server::version::Version;

pub trait CurrentOs: fmt::Debug + Send + Sync + 'static {
    fn get_type_name(&self) -> &'static str {
        type_name::<Self>()
    }
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods>;
    fn detect_all(&self) -> serde_json::Value;
    fn make_method<'x>(
        &'x self,
        method: &InstallMethod,
        methods: &InstallationMethods,
    ) -> anyhow::Result<Box<dyn Method + 'x>>;
}

pub trait Instance: fmt::Debug {
    fn name(&self) -> &str;
    fn method(&self) -> &dyn Method;
    fn get_version(&self) -> anyhow::Result<&MajorVersion>;
    fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>>;
    fn get_port(&self) -> anyhow::Result<u16>;
    fn get_start_conf(&self) -> anyhow::Result<StartConf>;
    fn get_status(&self) -> Status;
    fn start(&self, start: &Start) -> anyhow::Result<()>;
    fn stop(&self, stop: &Stop) -> anyhow::Result<()>;
    fn restart(&self, restart: &Restart) -> anyhow::Result<()>;
    fn logs(&self, logs: &Logs) -> anyhow::Result<()>;
    fn service_status(&self) -> anyhow::Result<()>;
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder>;
    fn get_command(&self) -> anyhow::Result<Command>;
    fn upgrade(&self, meta: &Metadata) -> anyhow::Result<InstanceRef<'_>>;
    fn revert(&self, metadata: &Metadata) -> anyhow::Result<()>;
    fn into_ref<'x>(self) -> InstanceRef<'x>
    where
        Self: Sized + 'x,
    {
        InstanceRef(Box::new(self))
    }
}

#[derive(Debug)]
pub struct InstanceRef<'a>(Box<dyn Instance + 'a>);

pub trait Method: fmt::Debug + Send + Sync {
    fn name(&self) -> InstallMethod;
    fn install(&self, settings: &install::Settings) -> anyhow::Result<()>;
    fn all_versions(&self, nightly: bool) -> anyhow::Result<Vec<DistributionRef>>;
    fn get_version(&self, query: &VersionQuery) -> anyhow::Result<DistributionRef>;
    fn installed_versions(&self) -> anyhow::Result<Vec<DistributionRef>>;
    fn detect_all(&self) -> serde_json::Value;
    fn is_system_only(&self) -> bool {
        false
    }
    fn get_storage(&self, system: bool, name: &str) -> anyhow::Result<Storage>;
    fn storage_exists(&self, storage: &Storage) -> anyhow::Result<bool>;
    fn clean_storage(&self, storage: &Storage) -> anyhow::Result<()>;
    fn bootstrap(&self, settings: &init::Settings) -> anyhow::Result<()>;
    fn upgrade(&self, todo: &upgrade::ToDo, options: &Upgrade) -> anyhow::Result<()>;
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>>;
    fn get_instance<'x>(&'x self, name: &str) -> anyhow::Result<InstanceRef<'x>>;
    fn destroy(&self, options: &Destroy) -> anyhow::Result<()>;
    fn uninstall(&self, distr: &DistributionRef) -> anyhow::Result<()>;
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

impl InstanceRef<'_> {
    pub fn name(&self) -> &str {
        self.0.name()
    }
    pub fn get_version(&self) -> anyhow::Result<&MajorVersion> {
        self.0.get_version()
    }
    pub fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>> {
        self.0.get_current_version()
    }
    pub fn get_status(&self) -> Status {
        self.0.get_status()
    }
    pub fn get_port(&self) -> anyhow::Result<u16> {
        self.0.get_port()
    }
    pub fn get_start_conf(&self) -> anyhow::Result<StartConf> {
        self.0.get_start_conf()
    }
    pub fn start(&self, start: &Start) -> anyhow::Result<()> {
        self.0.start(start)
    }
    pub fn stop(&self, stop: &Stop) -> anyhow::Result<()> {
        self.0.stop(stop)
    }
    pub fn restart(&self, restart: &Restart) -> anyhow::Result<()> {
        self.0.restart(restart)
    }
    pub fn logs(&self, logs: &Logs) -> anyhow::Result<()> {
        self.0.logs(logs)
    }
    pub fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        self.0.get_connector(admin)
    }
    pub fn get_command(&self) -> anyhow::Result<Command> {
        self.0.get_command()
    }
    pub fn service_status(&self) -> anyhow::Result<()> {
        self.0.service_status()
    }
    pub fn upgrade(&self, meta: &Metadata) -> anyhow::Result<InstanceRef<'_>> {
        self.0.upgrade(meta)
    }
    pub fn revert(&self, metadata: &Metadata) -> anyhow::Result<()> {
        self.0.revert(metadata)
    }
}

impl<'a> AsRef<dyn Instance + 'a> for InstanceRef<'a> {
    fn as_ref(&self) -> &(dyn Instance + 'a) {
        &*self.0
    }
}
