use std::fs;
use std::env;

use anyhow::Context;

pub fn get_cli() -> anyhow::Result<&'static str> {
    if cfg!(target_arch="x86_64") {
        if cfg!(target_os="macos") {
            return Ok("x86_64-apple-darwin");
        } else if cfg!(target_os="linux") {
            return Ok("x86_64-unknown-linux-musl");
        } else if cfg!(windows) {
            return Ok("x86_64-pc-windows-msvc");
        } else {
            anyhow::bail!("unsupported OS on x86_64");
        }
    } else if cfg!(target_arch="aarch64") {
        if cfg!(target_os="macos") {
            return Ok("aarch64-apple-darwin");
        } else if cfg!(target_os="linux") {
            return Ok("aarch64-unknown-linux-musl");
        } else {
            anyhow::bail!("unsupported OS on aarch64")
        }
    } else {
        anyhow::bail!("unsupported architecture");
    }
}

pub fn get_server() -> anyhow::Result<&'static str> {
    if cfg!(target_arch="x86_64") {
        if cfg!(target_os="macos") {
            return Ok("x86_64-apple-darwin");
        } else if cfg!(target_os="linux") {
            return Ok("x86_64-unknown-linux-gnu");
        } else if cfg!(windows) {
            // on windows use server version from linux
            // as we run server in WSL
            return Ok("x86_64-unknown-linux-gnu");
        } else {
            anyhow::bail!("unsupported OS on x86_64");
        }
    } else if cfg!(target_arch="aarch64") {
        if cfg!(target_os="macos") {
            return Ok("aarch64-apple-darwin");
        } else if cfg!(target_os="linux") {
            return Ok("aarch64-unknown-linux-gnu");
        } else {
            anyhow::bail!("unsupported OS on aarch64")
        }
    } else {
        anyhow::bail!("unsupported architecture");
    }
}

fn docker_check() -> anyhow::Result<bool> {
    let cgroups = fs::read_to_string("/proc/self/cgroup")
        .context("cannot read /proc/self/cgroup")?;
    for line in cgroups.lines() {
        let mut fields = line.split(':');
        if fields.nth(2).map(|f| f.starts_with("/docker/")).unwrap_or(false) {
            return Ok(true);
        }
    }
    return Ok(false)
}

pub fn optional_docker_check() -> anyhow::Result<bool> {
    if cfg!(target_os="linux") {
       match env::var("EDGEDB_INSTALL_IN_DOCKER").as_ref().map(|x| &x[..]) {
            Ok("forbid") | Ok("default") | Err(env::VarError::NotPresent) => {
                let result = docker_check()
                    .map_err(|e| {
                        log::warn!("Failed to check if running within \
                                   a container: {:#}", e)
                    }).unwrap_or(false);
                return Ok(result);
            }
            Ok("allow") => return Ok(false),
            Ok(value) => {
                anyhow::bail!("Invalid value of \
                    EDGEDB_INSTALL_IN_DOCKER: {:?}. \
                    Options: allow, forbid, default.", value);
            }
            Err(env::VarError::NotUnicode(value)) => {
                anyhow::bail!("Invalid value of \
                    EDGEDB_INSTALL_IN_DOCKER: {:?}. \
                    Options: allow, forbid, default.", value);
            }
        };
    }
    Ok(false)
}
