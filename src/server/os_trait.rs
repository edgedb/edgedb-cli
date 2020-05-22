use std::fmt;

use crate::server::install;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::detect::InstallationMethods;
use crate::server::install::InstallMethod;
use crate::server::package::PackageInfo;


pub trait CurrentOs: fmt::Debug + Send + Sync + 'static {
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods>;
    fn detect_all(&self) -> serde_json::Value;
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>;
    fn instantiate_methods<'x>(&'x self)
        -> anyhow::Result<Vec<(InstallMethod, Box<dyn Method + 'x>)>>
    {
        let avail = self.get_available_methods()?;
        let mut res = Vec::with_capacity(2);
        if avail.package.supported {
            res.push((InstallMethod::Package,
                      self.make_method(&InstallMethod::Package, &avail)?));
        }
        if avail.docker.supported {
            res.push((InstallMethod::Docker,
                      self.make_method(&InstallMethod::Docker, &avail)?));
        }
        Ok(res)
    }
}

pub trait Method: fmt::Debug + Send + Sync {
    fn install(&self, settings: &install::Settings) -> anyhow::Result<()>;
    fn all_versions(&self, nightly: bool) -> anyhow::Result<&[PackageInfo]>;
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>;
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]>;
    fn detect_all(&self) -> serde_json::Value;
}
