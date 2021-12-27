use std::fs;
use std::env;
use std::path::{PathBuf};

use anyhow::Context;
use fn_error_context::context;

use crate::platform::{home_dir, current_exe};
use crate::portable::destroy::InstanceNotFound;
use crate::portable::local::{InstanceInfo, runstate_dir};
use crate::portable::options::{StartConf, Logs};
use crate::portable::status::Service;
use crate::process;


pub fn unit_dir() -> anyhow::Result<PathBuf> {
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


pub fn create_service(info: &InstanceInfo)
    -> anyhow::Result<()>
{
    let name = &info.name;
    let unit_dir = unit_dir()?;
    fs::create_dir_all(&unit_dir)
        .with_context(|| format!("cannot create directory {:?}", unit_dir))?;
    let unit_name = unit_name(name);
    let unit_path = unit_dir.join(&unit_name);
    fs::write(&unit_path, systemd_unit(name, info)?)
        .with_context(|| format!("cannot write {:?}", unit_path))?;
    if preliminary_detect().is_some() {
        process::Native::new("systemctl", "systemctl", "systemctl")
            .arg("--user")
            .arg("daemon-reload")
            .run()
            .map_err(|e| log::warn!("failed to reload systemd daemon: {}", e))
            .ok();
    } else {
        anyhow::bail!("no systemd user daemon found")
    }
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
pub fn systemd_unit(name: &str, _info: &InstanceInfo) -> anyhow::Result<String>
{
    Ok(format!(r###"
[Unit]
Description=EdgeDB Database Service, instance {instance_name:?}
Documentation=https://edgedb.com/
After=syslog.target
After=network.target

[Service]
Type=notify

RuntimeDirectory=edgedb-{instance_name}
ExecStart={executable} instance start {instance_name} --managed-by=systemd
ExecReload=/bin/kill -HUP ${{MAINPID}}
KillMode=mixed
TimeoutSec=0

[Install]
WantedBy=default.target
    "###,
        instance_name=name,
        executable=current_exe()?.display(),
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

pub fn server_cmd(inst: &InstanceInfo) -> anyhow::Result<process::Native> {
    let data_dir = inst.data_dir()?;
    let server_path = inst.server_path()?;
    let mut pro = process::Native::new("edgedb", "edgedb", server_path);
    pro.env_default("EDGEDB_SERVER_LOG_LEVEL", "warn");
    pro.env_default("EDGEDB_SERVER_HTTP_ENDPOINT_SECURITY", "optional");
    pro.env_default("EDGEDB_SERVER_INSTANCE_NAME", &inst.name);
    pro.arg("--data-dir").arg(data_dir);
    pro.arg("--runstate-dir").arg(runstate_dir(&inst.name)?);
    pro.arg("--port").arg(inst.port.to_string());
    Ok(pro)
}

pub fn detect_systemd(instance: &str) -> bool {
    _detect_systemd(instance).is_some()
}

fn preliminary_detect() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| env::var_os("DBUS_SESSION_BUS_ADDRESS"))?;
    if let Ok(path) = which::which("systemctl") {
        Some(path)
    } else {
        None
    }
}

fn _detect_systemd(instance: &str) -> Option<PathBuf> {
    let path = preliminary_detect()?;
    let unit_name = unit_name(instance);
    let out = process::Native::new("detect systemd", "systemctl", &path)
        .arg("--user")
        .arg("is-enabled")
        .arg(&unit_name)
        .get_output().ok()?;
    if out.status.success() {
        return Some(path);
    }
    if !out.stderr.is_empty() {
        log::info!("cannot access systemd daemon: {:?}",
                   String::from_utf8_lossy(&out.stderr));
        return None;
    }
    log::debug!("service is-enabled returned: {:?}",
                String::from_utf8_lossy(&out.stdout));
    return Some(path);
}

pub fn start_service(inst: &InstanceInfo) -> anyhow::Result<()> {
    process::Native::new("service start", "systemctl", "systemctl")
        .arg("--user")
        .arg("start")
        .arg(&unit_name(&inst.name))
        .run()?;
    Ok(())
}

pub fn stop_service(name: &str) -> anyhow::Result<()> {
    process::Native::new("stop service", "systemctl", "systemctl")
        .arg("--user")
        .arg("stop")
        .arg(unit_name(&name))
        .run()?;
    Ok(())
}

pub fn restart_service(inst: &InstanceInfo) -> anyhow::Result<()> {
    process::Native::new("restart service", "systemctl", "systemctl")
        .arg("--user")
        .arg("restart")
        .arg(unit_name(&inst.name))
        .run()?;
    Ok(())
}

pub fn service_status(name: &str) -> Service {
    use Service::*;

    let mut cmd = process::Native::new(
        "service status", "systemctl", "systemctl");
    cmd.arg("--user");
    cmd.arg("show");
    cmd.arg(unit_name(name));
    let txt = match cmd.get_stdout_text() {
        Ok(txt) => txt,
        Err(e) => {
            return Service::Inactive {
                error: format!("cannot determine service status: {:#}", e),
            }
        }
    };
    let mut pid = None;
    let mut exit = None;
    let mut load_error = None;
    for line in txt.lines() {
        if let Some(pid_str) = line.strip_prefix("MainPID=") {
            pid = pid_str.trim().parse().ok();
        }
        if let Some(status_str) = line.strip_prefix("ExecMainStatus=") {
            exit = status_str.trim().parse().ok();
        }
        if let Some(err) = line.strip_prefix("LoadError=") {
            load_error = Some(err.trim().to_string());
        }
    }
    match pid {
        None | Some(0) => {
            if let Some(error) = load_error {
                Inactive { error }
            } else {
                Failed { exit_code: exit }
            }
        }
        Some(pid) => {
            Running { pid }
        }
    }
}

pub fn external_status(inst: &InstanceInfo) -> anyhow::Result<()> {
    process::Native::new("service status", "systemctl", "systemctl")
        .arg("--user")
        .arg("status")
        .arg(unit_name(&inst.name))
        .no_proxy()
        .run_and_exit()?;
    Ok(())
}

pub fn logs(options: &Logs) -> anyhow::Result<()> {
    let mut cmd = process::Native::new(
        "logs", "journalctl", "journalctl");
    cmd.arg("--user-unit").arg(unit_name(&options.name));
    if let Some(n) = options.tail  {
        cmd.arg(format!("--lines={}", n));
    }
    if options.follow {
        cmd.arg("--follow");
    }
    cmd.no_proxy().run()
}

// We proxy for two reasons:
// 1. There is a race condition between sending notification and systemd
//    receiving it if we set NotifyAccess=all
// 2. For systemd user daemon in Docker, NotifyAccess doesn't work at all
//    (it looks like just because systemd fails to match cgroups that include
//    parent cgroup path)
#[cfg(target_os="linux")]
pub fn run_and_proxy_notify_socket(meta: &InstanceInfo) -> anyhow::Result<()> {
    use async_std::os::unix::net::UnixDatagram;
    use async_std::task;

    let systemd_socket = env::var_os("NOTIFY_SOCKET").unwrap();
    let systemd = task::block_on(async {
        let sock = UnixDatagram::unbound()
            .context("cannot create systemd notify socket")?;
        sock.connect(&systemd_socket).await
            .context("cannot connect to systemd notify socket")?;
        Ok::<_, anyhow::Error>(sock)
    })?;

    let inner_socket = runstate_dir(&meta.name)?.join(".s.nfy-inner");
    if inner_socket.exists() {
        fs::remove_file(&inner_socket)?;
    } else if let Some(dir) = inner_socket.parent() {
        fs::create_dir_all(dir)?;
    }
    let inner = task::block_on(UnixDatagram::bind(&inner_socket))
        .context("cannot create inner notify socket")?;
    server_cmd(&meta)?
        .env_default("EDGEDB_SERVER_LOG_LEVEL", "info")
        .env("NOTIFY_SOCKET", inner_socket)
        .no_proxy()
        .background_for(async {
            let mut buf = [0u8; 16384];
            loop {
                match inner.recv(&mut buf).await {
                    Ok(len) => {
                        systemd.send(&buf[..len]).await.ok();
                    }
                    Err(e) => {
                        log::warn!(
                            "Error receiving from notify socket: {:#}", e);
                    }
                }
            }
        })
}

#[cfg(not(target_os="linux"))]
pub fn run_and_proxy_notify_socket(_: &InstanceInfo) -> anyhow::Result<()> {
    unreachable!();
}
