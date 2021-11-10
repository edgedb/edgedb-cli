use std::fs;
use std::path::PathBuf;

use crate::platform::{home_dir, get_current_uid, cache_dir};
use crate::process;
use crate::portable::local::{Paths};
use crate::portable::create::{InstanceInfo};
use crate::portable::status::Service;
use crate::server::options::StartConf;


fn plist_dir() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join("Library/LaunchAgents"))
}

fn plist_name(name: &str) -> String {
    format!("com.edgedb.edgedb-server-{}.plist", name)
}

fn plist_path(name: &str) -> anyhow::Result<PathBuf> {
    Ok(plist_dir()?.join(plist_name(name)))
}

fn get_domain_target() -> String {
    format!("gui/{}", get_current_uid())
}

fn launchd_name(name: &str) -> String {
    format!("{}/edgedb-server-{}", get_domain_target(), name)
}

pub fn service_files(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    Ok(vec![plist_path(name)?])
}

pub fn create_service(name: &str, info: &InstanceInfo, paths: &Paths)
    -> anyhow::Result<()>
{
    // bootout on upgrade
    if is_service_loaded(name) {
        bootout(name)?;
    }

    if info.start_conf == StartConf::Auto {
        _create_service(name, info, paths)
    } else {
        Ok(())
    }
}

fn runtime_base() -> anyhow::Result<PathBuf> {
    Ok(cache_dir()?.join("run"))
}

fn runtime_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(runtime_base()?.join(name))
}

fn log_file(name: &str) -> anyhow::Result<PathBuf> {
    Ok(cache_dir()?.join(format!("logs/{}.log", name)))
}

fn plist_data(name: &str, info: &InstanceInfo, paths: &Paths)
    -> anyhow::Result<String>
{
    Ok(format!(r###"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN"
        "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>edgedb-server-{instance_name}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{server_path}</string>
        <string>--data-dir={data_dir}</string>
        <string>--runstate-dir={runtime_dir}</string>
        <string>--port={port}</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>EDGEDB_SERVER_INSTANCE_NAME</key>
        <string>{instance_name}</string>
        <key>EDGEDB_SERVER_ALLOW_INSECURE_HTTP_CLIENTS</key>
        <string>1</string>
    </dict>

    <key>StandardOutPath</key>
    <string>{log_path}</string>
    <key>StandardErrorPath</key>
    <string>{log_path}</string>

    <key>KeepAlive</key>
    <dict>
         <key>SuccessfulExit</key>
         <false/>
    </dict>
</dict>
</plist>
"###,
        instance_name=name,
        data_dir=paths.data_dir.display(),
        server_path=info.installation.server_path()?.display(),
        runtime_dir=runtime_dir(&name)?.display(),
        log_path=log_file(&name)?.display(),
        port=info.port,
    ))
}

fn _create_service(name: &str, info: &InstanceInfo, paths: &Paths)
    -> anyhow::Result<()>
{
    let plist_dir_path;
    let tmpdir;
    if info.start_conf == StartConf::Auto {
        plist_dir_path = plist_dir()?;
        fs::create_dir_all(&plist_dir_path)?;
    } else {
        tmpdir = tempfile::tempdir()?;
        plist_dir_path = tmpdir.path().to_path_buf();
    }
    let plist_path = plist_dir_path.join(&plist_name(name));
    let unit_name = launchd_name(name);
    fs::write(&plist_path, plist_data(name, info, paths)?)?;
    fs::create_dir_all(runtime_base()?)?;

    // Clear the disabled status of the unit name, in case the user disabled
    // a service with the same name some time ago and it's likely forgotten
    // because the user is now creating a new service with the same name.
    // This doesn't make the service auto-starting, because we're "hiding" the
    // plist file from launchd if the service is configured as manual start.
    // Actually it is necessary to clear the disabled status even for manually-
    // starting services, because manual start won't work on disabled services.
    process::Native::new("create service", "launchctl", "launchctl")
        .arg("enable").arg(&unit_name)
        .run()?;
    process::Native::new("create service", "launchctl", "launchctl")
        .arg("bootstrap")
        .arg(get_domain_target())
        .arg(plist_path)
        .run()?;

    Ok(())
}

fn bootout(name: &str) -> anyhow::Result<()> {
    let unit_name = launchd_name(name);
    let status = process::Native::new(
        "remove service", "launchctl", "launchctl")
        .arg("bootout").arg(&unit_name)
        .status()?;
    if status.success() {
        Ok(())
    } else if status.code() == Some(36) {
        // MacOS Catalina has a bug of returning:
        //   Boot-out failed: 36: Operation now in progress
        // when process has successfully booted out
        Ok(())
    } else {
        anyhow::bail!("launchctl bootout failed: {}", status)
    }
}

pub fn is_service_loaded(name: &str) -> bool {
    match service_status(name) {
        Service::Inactive {..} => false,
        _ => true,
    }
}

pub fn service_status(name: &str) -> Service {
    use Service::*;

    let list = process::Native::new("service list", "launchctl", "launchctl")
            .arg("list")
            .get_stdout_text();
    let txt = match list {
        Ok(txt) => txt,
        Err(e) => {
            return Service::Inactive {
                error: format!("cannot determine service status: {:#}", e),
            }
        }
    };
    let svc_name = format!("edgedb-server-{}", name);
    for line in txt.lines() {
        let mut iter = line.split_whitespace();
        let pid = iter.next().unwrap_or("-");
        let exit_code = iter.next();
        let cur_name = iter.next();
        if let Some(cur_name) = cur_name {
            if cur_name == svc_name {
                if pid == "-" {
                    return Failed {
                        exit_code: exit_code.and_then(|v| v.parse().ok()),
                    };
                }
                match pid.parse() {
                    Ok(pid) => return Running { pid },
                    Err(e) => return Inactive {
                        error: format!("invalid pid {:?}: {}", pid, e),
                    },
                }
            }
        }
    }
    Inactive { error: format!("service {:?} not found", svc_name) }
}

pub fn stop_and_disable(name: &str) -> anyhow::Result<bool> {
    if is_service_loaded(&name) {
        // bootout will fail if the service is not loaded (e.g. manually-
        // starting services that never started after reboot), also it's
        // unnecessary to unload the service if it wasn't loaded.
        log::info!("Unloading service");
        bootout(&name)?;
    }

    let mut found = false;
    let unit_path = plist_path(&name)?;
    if unit_path.exists() {
        found = true;
        log::info!("Removing unit file {}", unit_path.display());
        fs::remove_file(unit_path)?;
    }
    Ok(found)
}


