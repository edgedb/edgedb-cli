use std::path::PathBuf;

use async_std::task;
use edgedb_client::Builder;

use crate::platform::config_dir;



pub fn get_connector(name: &str) -> anyhow::Result<Builder> {
    task::block_on(Builder::read_credentials(path(name)?))
}

pub fn base_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("credentials"))
}

pub fn path(name: &str) -> anyhow::Result<PathBuf> {
    Ok(base_dir()?.join(format!("{}.json", name)))
}
