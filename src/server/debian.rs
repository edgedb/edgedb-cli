use serde::Serialize;

use crate::server::detect::{InstallationMethods};
use crate::server::install;
use crate::server::debian_like;
use crate::server::linux;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::PackageMethod;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};


#[derive(Debug, Serialize)]
pub struct Debian {
    #[serde(flatten)]
    linux: linux::Linux,
    #[serde(flatten)]
    common: debian_like::Debian,
}

impl Debian {
    pub fn new(rel: &os_release::OsRelease) -> anyhow::Result<Debian> {
        Ok(Debian {
            common: debian_like::Debian::new(
                "Debian", rel.version_codename.clone()),
            linux: linux::Linux::new(),
        })
    }
}

impl CurrentOs for Debian {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        self.common.get_available_methods()
    }
    fn detect_all(&self) -> serde_json::Value {
        self.linux.detect_all();
        serde_json::to_value(self).expect("can serialize")
    }
    fn make_method<'x>(&'x self, method: &install::InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use install::InstallMethod::*;

        match method {
            Package => Ok(Box::new(methods.package.make_method(self)?)),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

impl<'os> Method for PackageMethod<'os, Debian> {
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        linux::perform_install(
            self.os.common.install_operations(settings)?,
            &self.os.linux)
    }
    fn all_versions(&self) -> anyhow::Result<&[VersionResult]> {
        todo!();
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>
    {
        let packages = self.os.common.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        linux::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]> {
        todo!();
    }
    fn detect_all(&self) -> serde_json::Value {
        todo!();
    }
}
