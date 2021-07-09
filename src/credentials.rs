use std::path::PathBuf;
use fs_err as fs;

use async_std::task;
use edgedb_client::Builder;
use edgedb_client::credentials::Credentials;

use crate::platform::config_dir;
use crate::server::reset_password::write_credentials;



pub fn get_connector(name: &str) -> anyhow::Result<Builder> {
    task::block_on(Builder::read_credentials(path(name)?))
}

pub fn base_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("credentials"))
}

pub fn path(name: &str) -> anyhow::Result<PathBuf> {
    Ok(base_dir()?.join(format!("{}.json", name)))
}

pub fn add_certificate(instance_name: &str, certificate: &str)
    -> anyhow::Result<()>
{
    let cred_path = path(instance_name)?;
    let data = fs::read(&cred_path)?;
    let mut creds: Credentials = serde_json::from_slice(&data)?;
    creds.tls_cert_data = Some(certificate.into());
    write_credentials(&cred_path, &creds)?;
    Ok(())
}
