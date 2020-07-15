use std::path::PathBuf;

use serde::Serialize;

use crate::server::debian_like;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::install;
use crate::server::init;
use crate::server::linux;
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::{self, PackageMethod, PackageInfo};
use crate::server::version::Version;


#[derive(Debug, Serialize)]
pub struct Ubuntu {
    #[serde(flatten)]
    linux: linux::Linux,
    #[serde(flatten)]
    common: debian_like::Debian,
}

impl Ubuntu {
    pub fn new(rel: &os_release::OsRelease) -> anyhow::Result<Ubuntu> {
        Ok(Ubuntu {
            linux: linux::Linux::new(),
            common: debian_like::Debian::new(
                "Ubuntu", rel.version_codename.clone()),
        })
    }
}

impl CurrentOs for Ubuntu {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        self.common.get_available_methods()
    }
    fn detect_all(&self) -> serde_json::Value {
        self.linux.detect_all();
        serde_json::to_value(self).expect("can serialize")
    }
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use InstallMethod::*;
        match method {
            Package => Ok(Box::new(methods.package.make_method(self)?)),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

impl<'os> Method for PackageMethod<'os, Ubuntu> {
    fn name(&self) -> InstallMethod {
        InstallMethod::Package
    }
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        linux::perform_install(
            self.os.common.install_operations(settings)?,
            &self.os.linux)
    }
    fn all_versions(&self, nightly: bool) -> anyhow::Result<&[PackageInfo]> {
        Ok(self.os.common.get_repo(nightly)?
            .map(|x| &x.packages[..]).unwrap_or(&[]))
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>
    {
        let packages = self.os.common.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        package::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]> {
        Ok(&self.installed.get_or_try_init(|| {
            debian_like::get_installed()
        })?)
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn get_server_path(&self, major_version: &Version<String>)
        -> anyhow::Result<PathBuf>
    {
        Ok(linux::get_server_path(major_version))
    }
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>
    {
        linux::create_systemd_service(settings, self)
    }
}
