use fs_err as fs;

use std::path::PathBuf;

use crate::credentials;
use crate::process;
use crate::portable::{windows, linux, macos};
use crate::portable::local::InstanceInfo;
use crate::print::{self, echo};
use crate::server::options::{Start, Stop, Restart, InstanceCommand, Logs};


pub fn fallback(name: &str, success_message: &str,
                cmd: &InstanceCommand) -> anyhow::Result<()> {
    echo!("No instance", name, "found.",
          "Looking for deprecated instances...");
    crate::server::control::instance_command(cmd)?;
    eprintln!("{}", success_message);
    print::warn("Please upgrade instance to portable installation");
    Ok(())
}

pub fn do_start(inst: &InstanceInfo) -> anyhow::Result<()> {
    let cred_path = credentials::path(&inst.name)?;
    if !cred_path.exists() {
        log::warn!("No corresponding credentials file {:?} exists. \
                    Use `edgedb instance reset-password {}` to create one.",
                    cred_path, inst.name);
    }
    if cfg!(windows) {
        windows::start_service(inst)
    } else if cfg!(target_os="macos") {
        macos::start_service(inst)
    } else if cfg!(target_os="linux") {
        linux::start_service(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn get_server_cmd(inst: &InstanceInfo) -> anyhow::Result<process::Native> {
    if cfg!(windows) {
        windows::server_cmd(inst)
    } else if cfg!(target_os="macos") {
        macos::server_cmd(inst)
    } else if cfg!(target_os="linux") {
        linux::server_cmd(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn get_runstate_dir(name: &str) -> anyhow::Result<PathBuf> {
    if cfg!(windows) {
        windows::runstate_dir(&name)
    } else if cfg!(target_os="macos") {
        macos::runstate_dir(&name)
    } else if cfg!(target_os="linux") {
        linux::runstate_dir(&name)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn ensure_runstate_dir(name: &str) -> anyhow::Result<PathBuf> {
    let runstate_dir = get_runstate_dir(name)?;
    fs::create_dir_all(&runstate_dir)?;
    Ok(runstate_dir)
}

pub fn start(options: &Start) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        ensure_runstate_dir(&meta.name)?;
        if options.foreground {
            get_server_cmd(meta)?.no_proxy().run()
        } else {
            do_start(meta)
        }
    } else {
        fallback(&options.name, "Deprecated service started.",
                 &InstanceCommand::Start(options.clone()))
    }
}

pub fn do_stop(name: &str) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::stop_service(name)
    } else if cfg!(target_os="macos") {
        macos::stop_service(name)
    } else if cfg!(target_os="linux") {
        linux::stop_service(name)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn stop(options: &Stop) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        do_stop(&meta.name)
    } else {
        fallback(&options.name, "Deprecated service stopped.",
                 &InstanceCommand::Stop(options.clone()))
    }
}

pub fn do_restart(inst: &InstanceInfo) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::restart_service(inst)
    } else if cfg!(target_os="macos") {
        macos::restart_service(inst)
    } else if cfg!(target_os="linux") {
        linux::restart_service(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn restart(options: &Restart) -> anyhow::Result<()> {
    let meta = InstanceInfo::try_read(&options.name)?;
    if let Some(meta) = &meta {
        do_restart(meta)
    } else {
        fallback(&options.name, "Deprecated service restarted.",
                 &InstanceCommand::Restart(options.clone()))
    }
}

pub fn logs(options: &Logs) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::logs(options)
    } else if cfg!(target_os="macos") {
        macos::logs(options)
    } else if cfg!(target_os="linux") {
        linux::logs(options)
    } else {
        anyhow::bail!("unsupported platform");
    }
}
