use std::fs;
use std::process::Command;

use anyhow::Context;

use crate::process::{run, exit_from};
use crate::server::options::{Start, Stop, Restart, Status};
use crate::server::init::{data_path, Metadata};
use crate::server::methods::InstallMethod;
use crate::server::version::Version;


pub trait Instance {
    fn start(&mut self, options: &Start) -> anyhow::Result<()>;
    fn stop(&mut self, options: &Stop) -> anyhow::Result<()>;
    fn restart(&mut self, options: &Restart) -> anyhow::Result<()>;
    fn status(&mut self, options: &Status) -> anyhow::Result<()>;
}

pub struct SystemdInstance {
    name: String,
    #[allow(dead_code)]
    system: bool,
    #[allow(dead_code)]
    version: Version<String>,
}

pub fn get_instance(name: &str) -> anyhow::Result<Box<dyn Instance>> {
    let dir = data_path(false)?.join(name);
    if !dir.exists() {
        let sys_dir = data_path(true)?.join(name);
        if sys_dir.exists() {
            anyhow::bail!("System instances are not implemented yet");
        }
        anyhow::bail!("No instance {0:?} found. Run:\n  \
            edgedb server init {0}", name);
    }
    let metadata_path = dir.join("metadata.json");
    let metadata: Metadata = serde_json::from_slice(
        &fs::read(&metadata_path)
        .with_context(|| format!("failed to read metadata {}",
                                 metadata_path.display()))?)
        .with_context(|| format!("failed to read metadata {}",
                                 metadata_path.display()))?;
    match metadata.method {
        InstallMethod::Package if cfg!(target_os="linux") => {
            Ok(Box::new(SystemdInstance {
                name: name.to_owned(),
                system: false,
                version: metadata.version.to_owned(),
            }))
        }
        _ => {
            anyhow::bail!("Unknown installation method and OS combination");
        }
    }
}

impl Instance for SystemdInstance {
    fn start(&mut self, _options: &Start) -> anyhow::Result<()> {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("start")
            .arg(format!("edgedb@{}", self.name)))?;
        Ok(())
    }
    fn stop(&mut self, _options: &Stop) -> anyhow::Result<()> {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("stop")
            .arg(format!("edgedb@{}", self.name)))?;
        Ok(())
    }
    fn restart(&mut self, _options: &Restart) -> anyhow::Result<()> {
        run(Command::new("systemctl")
            .arg("--user")
            .arg("restart")
            .arg(format!("edgedb@{}", self.name)))?;
        Ok(())
    }
    fn status(&mut self, _options: &Status) -> anyhow::Result<()> {
        exit_from(Command::new("systemctl")
            .arg("--user")
            .arg("status")
            .arg(format!("edgedb@{}", self.name)))?;
        Ok(())
    }
}
