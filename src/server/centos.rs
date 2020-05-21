use std::fs;
use std::io;
use std::str;

use anyhow::Context;
use serde::Serialize;

use crate::server::detect::{InstallationMethods, Lazy, ARCH};
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::docker::DockerCandidate;
use crate::server::install::{self, Operation, Command};
use crate::server::linux;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::PackageMethod;
use crate::server::package::{RepositoryInfo, PackageCandidate};


#[derive(Debug, Serialize)]
pub struct Centos {
    release: u32,
    #[serde(flatten)]
    linux: linux::Linux,
    #[serde(skip)]
    stable_repo: Lazy<Option<RepositoryInfo>>,
    #[serde(skip)]
    nightly_repo: Lazy<Option<RepositoryInfo>>,
}

fn repo_file(nightly: bool) -> &'static str {
    if nightly {
        "/etc/yum.repos.d/edgedb-server-install-nightly.repo"
    } else {
        "/etc/yum.repos.d/edgedb-server-install.repo"
    }
}

fn repo_data(nightly: bool) -> String {
    format!("\
            [edgedb-server-install{name_suffix}]\n\
            name=edgedb-server-install{name_suffix}\n\
            baseurl=https://packages.edgedb.com/rpm/el$releasever{suffix}/\n\
            enabled=1\n\
            gpgcheck=1\n\
            gpgkey={keyfile}\n\
        ",
        name_suffix=if nightly { "-nightly" } else {""},
        suffix=if nightly { ".nightly" } else {""},
        keyfile=install::KEY_FILE_URL)
}

impl Centos {
    pub fn new(rel: &os_release::OsRelease) -> anyhow::Result<Centos> {
        let release = rel.version_id.parse()
            .with_context(|| {
                format!("Error parsing version {:?}", rel.version_id)
            })?;
        Centos::from_release(release)
    }
    pub fn from_release(release: u32) -> anyhow::Result<Centos> {
        Ok(Centos {
            release,
            linux: linux::Linux::new(),
            stable_repo: Lazy::lazy(),
            nightly_repo: Lazy::lazy(),
        })
    }
    fn get_repo(&self, nightly: bool)
        -> anyhow::Result<Option<&RepositoryInfo>>
    {
        if nightly {
            self.nightly_repo.get_or_fetch(|| {
                format!("https://packages.edgedb.com/rpm/.jsonindexes/\
                    el{}.nightly.json",
                    self.release)
            })
        } else {
            self.stable_repo.get_or_fetch(|| {
                format!("https://packages.edgedb.com/rpm/.jsonindexes/\
                    el{}.json",
                    self.release)
            })
        }
    }
    fn install_operations(&self, settings: &install::Settings)
        -> anyhow::Result<Vec<Operation>>
    {
        let mut operations = Vec::new();
        let repo_data = repo_data(settings.nightly);
        let repo_path = repo_file(settings.nightly);
        let update_list = match fs::read(&repo_path) {
            Ok(data) => {
                let data_text = str::from_utf8(&data).map(|x| x.trim());
                data_text != Ok(repo_data.trim())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => true,
            Err(e) => {
                log::warn!("Unable to read {}: {:#}. Will replace...",
                    repo_path, e);
                true
            }
        };
        if update_list {
            operations.push(Operation::WritePrivilegedFile {
                path: repo_path.into(),
                data: repo_data.into(),
            });
        }
        operations.push(Operation::PrivilegedCmd(
            Command::new("yum")
            .arg("-y")
            .arg("install")
            .arg(format!("{}-{}",
                settings.package_name, settings.major_version))
            .env("_EDGEDB_INSTALL_SKIP_BOOTSTRAP", "1")
        ));
        Ok(operations)
    }
}

impl CurrentOs for Centos {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        let version_supported = self.get_repo(false)?
            .map(|repo| repo.packages.iter().any(|p| {
                (p.basename == "edgedb" || p.basename == "edgedb-server")
                && p.architecture == ARCH
            }))
            .unwrap_or(false);
        Ok(InstallationMethods {
            package: PackageCandidate {
                supported: version_supported,
                distro_name: "CentOS".into(),
                distro_version: self.release.to_string(),
                distro_supported: true,
                version_supported,
            },
            docker: DockerCandidate::detect()?,
        })
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

impl<'os> Method for PackageMethod<'os, Centos> {
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        linux::perform_install(
            self.os.install_operations(settings)?,
            &self.os.linux)
    }
    fn all_versions(&self) -> anyhow::Result<&[VersionResult]> {
        todo!();
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>
    {
        let packages = self.os.get_repo(query.is_nightly())?
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
