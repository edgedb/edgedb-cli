use std::fs;
use std::io;
use std::path::{PathBuf};
use std::process::Command as StdCommand;
use std::str;

use async_std::task;
use anyhow::Context;
use serde::Serialize;

use crate::server::detect::{Lazy, ARCH};
use crate::server::detect::VersionQuery;
use crate::server::distribution::{DistributionRef, MajorVersion, Distribution};
use crate::server::docker::DockerCandidate;
use crate::server::install::{self, Operation, Command};
use crate::server::linux;
use crate::server::init::{self, Storage};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method, InstanceRef};
use crate::server::package::{RepositoryInfo, PackageCandidate};
use crate::server::package::{self, PackageMethod, Package};
use crate::server::remote;
use crate::server::unix;
use crate::server::version::Version;


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
            self.nightly_repo.get_or_try_init(|| {
                task::block_on(remote::get_json_opt(
                    &format!("https://packages.edgedb.com/rpm/.jsonindexes/\
                        el{}.nightly.json",
                        self.release),
                    "failed to fetch repository index"))
            }).map(|opt| opt.as_ref())
        } else {
            self.stable_repo.get_or_try_init(|| {
                task::block_on(remote::get_json_opt(
                    &format!("https://packages.edgedb.com/rpm/.jsonindexes/\
                        el{}.json",
                        self.release),
                    "failed to fetch repository index"))
            }).map(|opt| opt.as_ref())
        }
    }
    fn install_operations(&self, settings: &install::Settings)
        -> anyhow::Result<Vec<Operation>>
    {
        let pkg = settings.distribution.downcast_ref::<Package>()
            .context("invalid centos package")?;
        let mut operations = Vec::new();
        let nightly = settings.distribution.major_version().is_nightly();
        let repo_data = repo_data(nightly);
        let repo_path = repo_file(nightly);
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
            .arg(format!("edgedb-server-{}", pkg.slot))
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

fn split_on<'x>(s: &'x str, delimiter: char) -> (&'x str, &'x str) {
    if let Some(idx) = s.find(delimiter) {
        (&s[..idx], &s[idx+1..])
    } else {
        (&s, "")
    }
}

impl<'os> Method for PackageMethod<'os, Centos> {
    fn name(&self) -> InstallMethod {
        InstallMethod::Package
    }
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        linux::perform_install(
            self.os.install_operations(settings)?,
            &self.os.linux)
    }
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<DistributionRef>>
    {
        Ok(self.os.get_repo(nightly)?
            .map(|x| {
                x.packages.iter()
                .filter(|p| p.basename == "edgedb-server" && p.slot.is_some())
                .map(|p| p.into())
                .collect()
            }).unwrap_or_else(Vec::new))
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<DistributionRef>
    {
        let packages = self.os.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        package::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<Vec<DistributionRef>> {
        Ok(self.installed.get_or_try_init(|| {
            let mut cmd = StdCommand::new("yum");
            cmd.arg("--showduplicates");
            cmd.arg("list").arg("installed");
            cmd.arg("edgedb-*");
            let out = cmd.output()
                .context("cannot get installed packages")?;
            if out.status.code() == Some(1) {
                if str::from_utf8(&out.stderr)
                    .map(|x| x.contains("No matching Packages to list"))
                    .unwrap_or(false)
                {
                    return Ok(Vec::new());
                }
                anyhow::bail!("cannot get installed packages: {:?} {}",
                    cmd, out.status);
            } else if !out.status.success() {
                anyhow::bail!("cannot get installed packages: {:?} {}",
                    cmd, out.status);
            }
            let mut lines = out.stdout.split(|&b| b == b'\n');
            for line in &mut lines {
                if line == b"Installed Packages" {
                    break;
                }
            }
            let mut result = Vec::new();
            for line in lines {
                let mut it = match str::from_utf8(line) {
                    Ok(line) => line.split_whitespace(),
                    Err(_) => continue,
                };
                let (pkg, ver, nightly) =
                    match (it.next(), it.next(), it.next()) {
                        (Some(name), Some(ver),
                            Some("@edgedb-server-install"))
                        => (name, ver, false),
                        (Some(name), Some(ver),
                            Some("@edgedb-server-install-nightly"))
                        => (name, ver, true),
                        _ => continue,
                    };
                let (pkg_name, arch) = split_on(pkg, '.');
                if arch != ARCH {
                    continue;
                }
                let (_pkg_name, major_version) =
                    if pkg_name.starts_with("edgedb-server-") {
                        ("edgedb-server", &pkg_name["edgedb-server-".len()..])
                    } else {
                        ("edgedb", &pkg_name["edgedb-".len()..])
                    };

                if major_version.chars().next()
                   .map(|x| !x.is_digit(10)).unwrap_or(true)
                {
                    continue;
                }
                result.push(Package {
                    major_version: if nightly {
                        MajorVersion::Nightly
                    } else {
                        MajorVersion::Stable(Version(major_version.to_owned()))
                    },
                    version: Version(ver.to_owned()),
                    slot: major_version.to_owned(),
                }.into_ref());
            }
            Ok(result)
        })?.clone())
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn bootstrap(&self, init: &init::Settings) -> anyhow::Result<()> {
        unix::bootstrap(init)
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
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>
    {
        linux::create_systemd_service(settings, self)
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>> {
        linux::all_instances()
    }
}
