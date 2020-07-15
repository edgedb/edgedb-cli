use std::fmt;
use std::any::type_name;
use std::path::PathBuf;

use crate::server::install;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::package::PackageInfo;
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
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>;
}
