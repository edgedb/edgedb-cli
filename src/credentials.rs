use std::io;
use std::path::PathBuf;
use std::collections::BTreeSet;

use fs_err as fs;

use anyhow::Context;
use async_std::task;

use edgedb_client::Builder;
use edgedb_client::credentials::Credentials;

use crate::platform::config_dir;
use crate::question;
use crate::server::reset_password::write_credentials;
use crate::server::is_valid_name;


pub fn get_connector(name: &str) -> anyhow::Result<Builder> {
    let mut builder = Builder::uninitialized();
    task::block_on(builder.read_instance(name))?;
    Ok(builder)
}

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
                if is_valid_name(name) {
                    result.insert(name.into());
                }
            }
        }
    }
    Ok(result)
}

pub fn add_certificate(instance_name: &str, certificate: &str)
    -> anyhow::Result<()>
{
    let cred_path = path(instance_name)?;
    let data = fs::read(&cred_path)?;
    let mut creds: Credentials = serde_json::from_slice(&data)?;
    creds.tls_cert_data = Some(certificate.into());
    task::block_on(write_credentials(&cred_path, &creds))?;
    Ok(())
}

pub fn maybe_update_credentials_file(
    builder: &Builder, ask: bool
) -> anyhow::Result<()> {
    if let Some(instance_name) = builder.get_instance_name_for_creds_update() {
        let creds_path = path(instance_name)?;
        if !ask || question::Confirm::new(format!(
            "The format of the instance credential file at {} is outdated, \
             update now?",
            creds_path.display(),
        )).ask()? {
            task::block_on(write_credentials(
                &creds_path, &builder.as_credentials()?
            ))?;
        }
    }
    Ok(())
}
