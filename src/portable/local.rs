use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;
use fn_error_context::context;

use crate::credentials;
use crate::platform::{portable_dir, data_dir};
use crate::portable::ver;
use crate::portable::repository::PackageHash;
use crate::portable::{windows, linux, macos};
use crate::server::options::StartConf;


pub struct Paths {
    pub credentials: PathBuf,
    pub data_dir: PathBuf,
    pub service_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstanceInfo {
    #[serde(skip)]
    pub name: String,
    pub installation: InstallInfo,
    pub port: u16,
    pub start_conf: StartConf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallInfo {
    pub version: ver::Build,
    pub package_url: url::Url,
    pub package_hash: PackageHash,
    #[serde(with="serde_millis")]
    pub installed_at: SystemTime,
}


fn list_installed<'x>(dir: &'x Path)
    -> anyhow::Result<
        impl Iterator<Item=anyhow::Result<(ver::Specific, PathBuf)>> + 'x
    >
{
    let err_ctx = move || format!("error reading directory {:?}", dir);
    let dir = fs::read_dir(&dir).with_context(err_ctx)?;
    Ok(dir.filter_map(move |result| {
        let entry = match result {
            Ok(entry) => entry,
            res => return Some(Err(res.with_context(err_ctx).unwrap_err())),
        };
        let ver_opt = entry.file_name().to_str()
            .and_then(|x| x.parse().ok());
        if let Some(ver) = ver_opt {
            return Some(Ok((ver, entry.path())))
        } else {
            log::info!("Skipping directory {:?}", entry.path());
            return None
        }
    }))
}

#[context("cannot read {:?}", path)]
fn _read_install_info(path: &Path) -> anyhow::Result<InstallInfo> {
    let file = fs::File::open(path)?;
    let file = io::BufReader::new(file);
    Ok(serde_json::from_reader(file)?)
}

impl InstallInfo {
    pub fn read(dir: &Path) -> anyhow::Result<InstallInfo> {
        _read_install_info(&dir.join("install_info.json"))
    }
}

#[context("failed to list installed packages")]
pub fn get_installed() -> anyhow::Result<Vec<InstallInfo>> {
    let mut installed = Vec::with_capacity(8);
    let dir = portable_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    for result in list_installed(dir)? {
        let (ver, path) = result?;
        match InstallInfo::read(&path) {
            Ok(info) if ver != info.version.specific() => {
                log::warn!("Mismatching package version in {:?}: {} != {}",
                           path, info.version, ver);
                continue;
            }
            Ok(info) => installed.push(info),
            Err(e) => log::warn!("Skipping {:?}: {:#}", path, e),
        }
    }
    Ok(installed)
}

pub fn instance_data_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(data_dir()?.join(name))
}

impl Paths {
    pub fn get(name: &str) -> anyhow::Result<Paths> {
        Ok(Paths {
            credentials: credentials::path(name)?,
            data_dir: instance_data_dir(name)?,
            service_files: if cfg!(windows) {
                windows::service_files(name)?
            } else if cfg!(target_os="macos") {
                macos::service_files(name)?
            } else if cfg!(target_os="linux") {
                linux::service_files(name)?
            } else {
                Vec::new()
            }
        })
    }
    pub fn check_exists(&self) -> anyhow::Result<()> {
        if self.credentials.exists() {
            anyhow::bail!("Credentials file {:?} exists", self.credentials);
        }
        if self.data_dir.exists() {
            anyhow::bail!("Data directory {:?} exists", self.data_dir);
        }
        for path in &self.service_files {
            if path.exists() {
                anyhow::bail!("Service file {:?} exists", path);
            }
        }
        Ok(())
    }
}


impl InstanceInfo {
    pub fn try_read(name: &str) -> anyhow::Result<Option<InstanceInfo>> {
        let mut path = instance_data_dir(name)?;
        path.push("instance_info.json");
        // TODO(tailhook) check existence of the directory
        // and crash on existence of the file.
        // But this can only be done, once we get rid of old install methods
        if !path.exists() {
            return Ok(None)
        }
        Ok(Some(InstanceInfo::_read(name, &path)?))
    }

    pub fn read(name: &str) -> anyhow::Result<InstanceInfo> {
        InstanceInfo::_read(name,
            &instance_data_dir(name)?.join("instance_info.json"))
    }

    #[context("error reading instance info: {:?}", path)]
    fn _read(name: &str, path: &PathBuf) -> anyhow::Result<InstanceInfo> {
        let f = io::BufReader::new(fs::File::open(path)?);
        let mut data: InstanceInfo = serde_json::from_reader(f)?;
        data.name = name.into();
        Ok(data)
    }
    pub fn data_dir(&self) -> anyhow::Result<PathBuf> {
        instance_data_dir(&self.name)
    }
    pub fn server_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.installation.server_path()?)
    }
}

fn installation_path(ver: &ver::Specific) -> anyhow::Result<PathBuf> {
    Ok(portable_dir()?.join(ver.to_string()))
}

impl InstallInfo {
    pub fn base_path(&self) -> anyhow::Result<PathBuf> {
        Ok(installation_path(&self.version.specific())?)
    }
    pub fn server_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.base_path()?.join("bin").join("edgedb-server"))
    }
}
