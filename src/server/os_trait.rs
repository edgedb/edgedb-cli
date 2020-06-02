use std::fmt;
use std::path::PathBuf;

use crate::server::install;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::package::PackageInfo;
use crate::server::version::Version;


pub trait CurrentOs: fmt::Debug + Send + Sync + 'static {
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods>;
    fn detect_all(&self) -> serde_json::Value;
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>;
}

pub trait Method: fmt::Debug + Send + Sync {
    fn install(&self, settings: &install::Settings) -> anyhow::Result<()>;
    fn all_versions(&self, nightly: bool) -> anyhow::Result<&[PackageInfo]>;
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>;
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]>;
    fn detect_all(&self) -> serde_json::Value;
    fn is_system_only(&self) -> bool {
        false
    }
    fn get_server_path(&self, major_version: &Version<String>)
        -> anyhow::Result<PathBuf>;
}
