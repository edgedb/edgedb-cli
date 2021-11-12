use std::path::PathBuf;

use crate::portable::local::{InstanceInfo};
use crate::portable::status::{Service};
use crate::server::options::Start;


pub fn service_files(_name: &str) -> anyhow::Result<Vec<PathBuf>> {
    // TODO(tailhook)
    Ok(Vec::new())
}

pub fn create_service(_name: &str, _info: &InstanceInfo) -> anyhow::Result<()>
{
    anyhow::bail!("auto-start is not supported on Windows yet");
}

pub fn stop_and_disable(_name: &str) -> anyhow::Result<bool> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn start_service(_options: &Start, _inst: &InstanceInfo)
    -> anyhow::Result<()>
{
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn stop_service(_inst: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn restart_service(_inst: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}

pub fn service_status(_inst: &str) -> Service {
    Service::Inactive {
        error: "running as a service is not supported on Windows yet".into(),
    }
}

pub fn external_status(_inst: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("running as a service is not supported on Windows yet");
}
