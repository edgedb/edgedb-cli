use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fn_error_context::context;

use crate::platform::{home_dir, get_current_uid};
use crate::portable::local::{InstanceInfo};
use crate::process;
use crate::server::errors::InstanceNotFound;
use crate::server::options::StartConf;
use crate::server::options::Start;


fn unit_dir() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".config/systemd/user"))
}

fn unit_name(name: &str) -> String {
    format!("edgedb-server@{}.service", name)
}

fn systemd_service_path(name: &str) -> anyhow::Result<PathBuf> {
    Ok(unit_dir()?.join(unit_name(name)))
}

pub fn service_files(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    Ok(vec![systemd_service_path(name)?])
}


pub fn create_service(name: &str, info: &InstanceInfo)
    -> anyhow::Result<()>
{
    let unit_dir = unit_dir()?;
    fs::create_dir_all(&unit_dir)
        .with_context(|| format!("cannot create directory {:?}", unit_dir))?;
    let unit_name = unit_name(name);
    let unit_path = unit_dir.join(&unit_name);
    fs::write(&unit_path, systemd_unit(name, info)?)
        .with_context(|| format!("cannot write {:?}", unit_path))?;
    process::Native::new("systemctl", "systemctl", "systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .run()
        .map_err(|e| log::warn!("failed to reload systemd daemon: {}", e))
        .ok();
    if info.start_conf == StartConf::Auto {
        process::Native::new("systemctl", "systemctl", "systemctl")
            .arg("--user")
            .arg("enable")
            .arg(&unit_name)
            .run()?;
        process::Native::new("systemctl", "systemctl", "systemctl")
            .arg("--user")
            .arg("start")
            .arg(&unit_name)
            .run()?;
    }
    Ok(())
}

#[context("cannot compose service file")]
pub fn systemd_unit(name: &str, info: &InstanceInfo) -> anyhow::Result<String>
{
    Ok(format!(r###"
[Unit]
Description=EdgeDB Database Service, instance {instance_name:?}
Documentation=https://edgedb.com/
After=syslog.target
After=network.target

[Service]
Type=notify

Environment="EDGEDATA={data_dir}" "EDGEDB_SERVER_INSTANCE_NAME={instance_name}" "EDGEDB_SERVER_ALLOW_INSECURE_HTTP_CLIENTS=1"
RuntimeDirectory=edgedb-{instance_name}

ExecStart={server_path} --data-dir=${{EDGEDATA}} --runstate-dir=%t/edgedb-{instance_name} --port={port}
ExecReload=/bin/kill -HUP ${{MAINPID}}
KillMode=mixed
TimeoutSec=0

[Install]
WantedBy=multi-user.target
    "###,
        instance_name=name,
        data_dir=info.data_dir()?.display(),
        server_path=info.server_path()?.display(),
        port=info.port,
    ))
}

fn systemd_is_not_found_error(e: &str) -> bool {
    e.contains("Failed to get D-Bus connection") ||
    e.contains("Failed to connect to bus") ||
    e.contains("No such file or directory") ||
    e.contains(".service not loaded") ||
    e.contains(".service does not exist")
}

pub fn stop_and_disable(name: &str) -> anyhow::Result<bool> {
    let mut found = false;
    let svc_name = unit_name(name);
    log::info!("Stopping service {}", svc_name);
    let mut not_found_error = None;
    let mut cmd = process::Native::new(
        "stop service", "systemctl", "systemctl");
    cmd.arg("--user");
    cmd.arg("stop");
    cmd.arg(&svc_name);
    match cmd.run_or_stderr()? {
        Ok(()) => found = true,
        Err((_, e)) if systemd_is_not_found_error(&e) => {
            not_found_error = Some(e);
        }
        Err((s, e)) => {
            log::warn!(
                "Error running systemctl (command-line: {:?}): {}: {}",
                cmd.command_line(), s, e);
        }
    }

    let mut cmd = process::Native::new(
        "disable service", "systemctl", "systemctl");
    cmd.arg("--user");
    cmd.arg("disable");
    cmd.arg(&svc_name);
    match cmd.run_or_stderr()? {
        Ok(()) => found = true,
        Err((_, e)) if systemd_is_not_found_error(&e) => {
            not_found_error = Some(e);
        }
        Err((s, e)) => {
            log::warn!(
                "Error running systemctl (command-line: {:?}): {}: {}",
                cmd.command_line(), s, e);
        }
    }
    if let Some(e) = not_found_error {
        return Err(InstanceNotFound(anyhow::anyhow!(
            "no instance {:?} found: {}", name, e.trim())).into());
    }
    Ok(found)
}

pub fn start_service(options: &Start, inst: &InstanceInfo)
    -> anyhow::Result<()>
{
    if options.foreground {
        let data_dir = inst.data_dir()?;
        let runtime_dir = dirs::runtime_dir()
            .map(|r| r.join(format!("edgedb-{}", &inst.name)))
            .unwrap_or_else(|| {
                let dir = Path::new("/run/user")
                    .join(get_current_uid().to_string());
                if dir.exists() {
                    dir.join(format!("edgedb-{}", options.name))
                } else {
                    data_dir.clone()
                }
            });
        let server_path = inst.server_path()?;
        process::Native::new("edgedb", "edgedb", server_path)
            .env_default("EDGEDB_SERVER_LOG_LEVEL", "warn")
            .arg("--data-dir").arg(data_dir)
            .arg("--runstate-dir").arg(runtime_dir)
            .arg("--port").arg(inst.port.to_string())
            .no_proxy()
            .run()?;
    } else {
        process::Native::new("service start", "systemctl", "systemctl")
            .arg("--user")
            .arg("start")
            .arg(format!("edgedb-server@{}", options.name))
            .run()?;
    }
    Ok(())
}

pub fn stop_service(inst: &InstanceInfo) -> anyhow::Result<()> {
    process::Native::new("stop service", "systemctl", "systemctl")
        .arg("--user")
        .arg("stop")
        .arg(format!("edgedb-server@{}", inst.name))
        .run()?;
    Ok(())
}

pub fn restart_service(inst: &InstanceInfo) -> anyhow::Result<()> {
    process::Native::new("restart service", "systemctl", "systemctl")
        .arg("--user")
        .arg("restart")
        .arg(format!("edgedb-server@{}", inst.name))
        .run()?;
    Ok(())
}
