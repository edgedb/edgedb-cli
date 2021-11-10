use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fn_error_context::context;

use crate::credentials;
use crate::platform::{portable_dir, data_dir};
use crate::portable::install::InstallInfo;
use crate::portable::ver;
use crate::portable::{windows, linux, macos};


pub struct Paths {
    pub credentials: PathBuf,
    pub data_dir: PathBuf,
    pub service_files: Vec<PathBuf>,
}


fn opendir<'x>(dir: &'x Path)
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

pub fn read_metadata(dir: &Path) -> anyhow::Result<InstallInfo> {
    _read_metadata(&dir.join("install_info.json"))
}
#[context("cannot read {:?}", path)]
fn _read_metadata(path: &Path) -> anyhow::Result<InstallInfo> {
    let file = fs::File::open(path)?;
    let file = io::BufReader::new(file);
    Ok(serde_json::from_reader(file)?)
}

#[context("failed to list installed packages")]
pub fn get_installed() -> anyhow::Result<Vec<InstallInfo>> {
    let mut installed = Vec::with_capacity(8);
    for result in opendir(&portable_dir()?)? {
        let (ver, path) = result?;
        match read_metadata(&path) {
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

impl Paths {
    pub fn get(name: &str) -> anyhow::Result<Paths> {
        Ok(Paths {
            credentials: credentials::path(name)?,
            data_dir: data_dir()?.join(name),
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
