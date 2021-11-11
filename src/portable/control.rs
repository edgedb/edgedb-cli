use crate::portable::{windows, linux, macos};
use crate::portable::local::InstanceInfo;
use crate::print::{self, eecho};
use crate::server::options::{Start, Stop, Restart, InstanceCommand};


fn fallback(name: &str, cmd: &InstanceCommand) -> anyhow::Result<()> {
    eecho!("No instance", name, "found.",
           "Looking for deprecated instances...");
    crate::server::control::instance_command(cmd)?;
    print::warn("Please upgrade instance to portable installation");
    Ok(())
}


pub fn start(options: &Start) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        if cfg!(windows) {
            windows::start_service(options, meta)
        } else if cfg!(target_os="macos") {
            macos::start_service(options, meta)
        } else if cfg!(target_os="linux") {
            linux::start_service(options, meta)
        } else {
            anyhow::bail!("unsupported platform");
        }
    } else {
        fallback(&options.name, &InstanceCommand::Start(options.clone()))
    }
}

pub fn stop(options: &Stop) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        if cfg!(windows) {
            windows::stop_service(meta)
        } else if cfg!(target_os="macos") {
            macos::stop_service(meta)
        } else if cfg!(target_os="linux") {
            linux::stop_service(meta)
        } else {
            anyhow::bail!("unsupported platform");
        }
    } else {
        fallback(&options.name, &InstanceCommand::Stop(options.clone()))
    }
}

pub fn restart(options: &Restart) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        if cfg!(windows) {
            windows::restart_service(meta)
        } else if cfg!(target_os="macos") {
            macos::restart_service(meta)
        } else if cfg!(target_os="linux") {
            linux::restart_service(meta)
        } else {
            anyhow::bail!("unsupported platform");
        }
    } else {
        fallback(&options.name, &InstanceCommand::Restart(options.clone()))
    }
}
