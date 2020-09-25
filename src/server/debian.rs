use serde::Serialize;

use crate::server::debian_like;
use crate::server::detect::VersionQuery;
use crate::server::distribution::DistributionRef;
use crate::server::init::{self, Storage};
use crate::server::install;
use crate::server::linux;
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method, InstanceRef};
use crate::server::options::{Upgrade};
use crate::server::package::{self, PackageMethod};
use crate::server::unix;
use crate::server::upgrade;


#[derive(Debug, Serialize)]
pub struct Debian {
    #[serde(flatten)]
    linux: linux::Linux,
    #[serde(flatten)]
    common: debian_like::Debian,
}

impl Debian {
    pub fn new(rel: &os_release::OsRelease) -> anyhow::Result<Debian> {
        let codename = match (rel.version.find("("), rel.version.find(")")) {
            (Some(start), Some(end)) => {
                &rel.version[start+1..end]
            }
            _ => "",
        };
        Ok(Debian {
            common: debian_like::Debian::new("Debian", codename.into()),
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

impl<'os> Method for PackageMethod<'os, Debian> {
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
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<DistributionRef>>
    {
        self.os.common.all_versions(nightly)
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<DistributionRef>
    {
        let packages = self.os.common.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        package::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<Vec<DistributionRef>> {
        Ok(self.installed.get_or_try_init(|| {
            debian_like::get_installed()
        })?.clone())
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn bootstrap(&self, init: &init::Settings) -> anyhow::Result<()> {
        unix::bootstrap(self, init)
    }
    fn get_storage(&self, system: bool, name: &str)-> anyhow::Result<Storage> {
        unix::storage(system, name)
    }
    fn storage_exists(&self, storage: &Storage) -> anyhow::Result<bool> {
        unix::storage_exists(storage)
    }
    fn clean_storage(&self, storage: &Storage) -> anyhow::Result<()> {
        unix::clean_storage(storage)
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>> {
        linux::all_instances(self)
    }
    fn get_instance<'x>(&'x self, name: &str)
        -> anyhow::Result<InstanceRef<'x>>
    {
        linux::get_instance(self, name)
    }
    fn upgrade(&self, todo: &upgrade::ToDo, options: &Upgrade)
        -> anyhow::Result<()>
    {
        unix::upgrade(todo, options, self)
    }
}
