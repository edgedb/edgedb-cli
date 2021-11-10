use std::path::PathBuf;

use crate::portable::local::{Paths};
use crate::portable::create::{InstanceInfo};


pub fn service_files(_name: &str) -> anyhow::Result<Vec<PathBuf>> {
    // TODO(tailhook)
    Ok(Vec::new())
}

pub fn create_service(_name: &str, _info: &InstanceInfo, _paths: &Paths)
    -> anyhow::Result<()>
{
    anyhow::bail!("auto-start is not supported on Windows yet");
}

pub fn stop_and_disable(_name: &str) -> anyhow::Result<bool> {
    todo!();
}

