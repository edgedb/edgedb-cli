use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use fn_error_context::context;

use crate::platform::home_dir;
use crate::portable::create::{Paths, InstanceInfo};
use crate::process;
use crate::server::options::StartConf;


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


pub fn create_service(name: &str, info: &InstanceInfo, paths: &Paths)
    -> anyhow::Result<()>
{
    let unit_dir = unit_dir()?;
    fs::create_dir_all(&unit_dir)
        .with_context(|| format!("cannot create directory {:?}", unit_dir))?;
    let unit_name = unit_name(name);
    let unit_path = unit_dir.join(&unit_name);
    fs::write(&unit_path, systemd_unit(name, info, paths)?)
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
    }
    Ok(())
}

#[context("cannot compose service file")]
pub fn systemd_unit(name: &str, info: &InstanceInfo, paths: &Paths)
    -> anyhow::Result<String>
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
        data_dir=paths.data_dir.display(),
        server_path=info.installation.server_path()?.display(),
        port=info.port,
    ))
}
