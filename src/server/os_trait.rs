use std::fmt;

use crate::server::install;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::detect::InstallationMethods;
use crate::server::install::InstallMethod;


pub trait CurrentOs: fmt::Debug + Send + Sync + 'static {
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods>;
    fn detect_all(&self) -> serde_json::Value;
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>;
}

pub trait Method: fmt::Debug + Send + Sync {
    fn install(&self, settings: &install::Settings) -> anyhow::Result<()>;
    fn all_versions(&self) -> anyhow::Result<&[VersionResult]>;
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>;
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]>;
    fn detect_all(&self) -> serde_json::Value;
}
