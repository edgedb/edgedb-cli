use std::io;
use std::path::{Path, PathBuf};
use std::collections::BTreeSet;

use anyhow::Context;
use fn_error_context::context;
use fs_err as fs;

use edgedb_tokio::Config;
use edgedb_tokio::credentials::Credentials;

use crate::platform::{config_dir, tmp_file_name};
use crate::question;
use crate::portable::local::is_valid_instance_name;


pub fn base_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("credentials"))
}

pub fn path(name: &str) -> anyhow::Result<PathBuf> {
    Ok(base_dir()?.join(format!("{}.json", name)))
}

pub fn all_instance_names() -> anyhow::Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    let dir = base_dir()?;
    let dir_entries = match fs::read_dir(&dir) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(result),
        Err(e) => return Err(e).context(format!("error reading {:?}", dir)),
    };
    for item in dir_entries {
        let item = item?;
        if let Ok(filename) = item.file_name().into_string() {
            if let Some(name) = filename.strip_suffix(".json") {
                if is_valid_instance_name(name, false) {
                    result.insert(name.into());
                }
            }
        }
    }
    Ok(result)
}

#[tokio::main]
#[context("cannot write credentials file {}", path.display())]
pub async fn write(path: &Path, credentials: &Credentials)
    -> anyhow::Result<()>
{
    use tokio::fs;

    fs::create_dir_all(path.parent().unwrap()).await?;
    let tmp_path = path.with_file_name(tmp_file_name(path));
    fs::write(&tmp_path, serde_json::to_vec_pretty(&credentials)?).await?;
    fs::rename(&tmp_path, path).await?;
    Ok(())
}

pub fn maybe_update_credentials_file(
    config: &Config, ask: bool
) -> anyhow::Result<()> {
    if config.is_creds_file_outdated() {
        if let Some(instance_name) = config.local_instance_name() {
            let creds_path = path(instance_name)?;
            if !ask || question::Confirm::new(format!(
                "The format of the instance credential file at {} is outdated, \
             update now?",
                creds_path.display(),
            )).ask()? {
                write(&creds_path, &config.as_credentials()?)?;
            }
        }
    }
    Ok(())
}
