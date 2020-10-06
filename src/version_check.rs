use std::env;
use std::io;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};

use anyhow::Context;
use async_std::future::Future;
use async_std::task;
use fn_error_context::context;
use rand::{thread_rng, Rng};
use serde::{Serialize, Deserialize};

use crate::platform::home_dir;
use crate::server::version::Version;
use crate::server::remote;
use crate::server::package::RepositoryInfo;


#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    #[serde(with="humantime_serde")]
    timestamp: SystemTime,
    #[serde(with="humantime_serde")]
    expires: SystemTime,
    version: Option<Version<String>>,
}

fn cache_age() -> Duration {
    Duration::from_secs(thread_rng().gen_range(16*3600, 32*3600))
}

fn negative_cache_age() -> Duration {
    Duration::from_secs(thread_rng().gen_range(6*3600, 12*3600))
}

#[allow(dead_code)]
fn can_update() -> bool {
    _can_update().unwrap_or_else(|e| {
        log::info!("Cannot compare current binary to default: {}", e);
        false
    })
}

fn _can_update() -> anyhow::Result<bool> {
    let dir = home_dir()?.join(".edgedb").join("bin");
    let default_path = if cfg!(windows) {
        dir.join("edgedb.exe")
    } else {
        dir.join("edgedb")
    };
    let exe_path = env::current_exe()
        .with_context(|| format!("cannot determine running executable path"))?;
    Ok(exe_path == default_path)
}

fn read_cache(dir: &Path) -> anyhow::Result<Cache> {
    let file = fs::File::open(dir.join("version_check.json"))?;
    Ok(serde_json::from_reader(file)?)
}

#[context("error writing {}/version_check.json", dir.display())]
fn write_cache(dir: &Path, data: &Cache) -> anyhow::Result<()> {
    let file = fs::File::create(dir.join("version_check.json"))?;
    Ok(serde_json::to_writer_pretty(file, data)?)
}

pub async fn timeout<F, T>(dur: Duration, f: F) -> anyhow::Result<T>
    where F: Future<Output = anyhow::Result<T>>,
{
    use async_std::future::timeout;

    timeout(dur, f).await
    .unwrap_or_else(|_| Err(io::Error::from(io::ErrorKind::TimedOut).into()))
}

fn _check(cache_dir: &Path) -> anyhow::Result<()> {
    match read_cache(cache_dir) {
        Ok(cache) if cache.expires > SystemTime::now() => {
            log::debug!("Cached version {:?}", cache.version);
            if let Some(ver) = cache.version {
                if Version(env!("CARGO_PKG_VERSION").into()) < ver {
                    log::warn!(
                        "Newer version of edgedb tool exists {} (current {})",
                        ver, env!("CARGO_PKG_VERSION"));
                }
            }
            return Ok(());
        }
        Ok(_) => {},
        Err(e) => {
            log::debug!("Error reading cache: {}", e);
        }
    }
    let platform =
        if cfg!(windows) {
            "windows"
        } else if cfg!(target_os="linux") {
            "linux"
        } else if cfg!(target_os="macos") {
            "macos"
        } else {
            anyhow::bail!("unknown OS");
        };
    let suffix = if env!("CARGO_PKG_VERSION").contains(".g") {
        ".nightly"
    } else {
        ""
    };
    let url = format!(
        "https://packages.edgedb.com/archive/.jsonindexes/{}-x86_64{}.json",
        platform, suffix
    );

    let timestamp = SystemTime::now();
    let repo: RepositoryInfo = match
        task::block_on(timeout(
            Duration::from_secs(1),
            remote::get_json(&url, "cannot get package index for CLI tools"),
        ))
    {
        Ok(repo) => repo,
        Err(e) => {
            log::info!("Error while checking for updates: {}", e);
            write_cache(cache_dir, &Cache {
                timestamp,
                expires: timestamp + negative_cache_age(),
                version: None,
            })?;
            return Ok(());
        }
    };
    let max = repo.packages.iter()
        .filter(|pkg| pkg.basename == "edgedb-cli")
        .map(|pkg| &pkg.version)
        .max();
    if let Some(ver) = &max {
        if Version(env!("CARGO_PKG_VERSION").into()) < **ver {
            log::warn!("Newer version of edgedb tool exists {} (current {})",
                ver, env!("CARGO_PKG_VERSION"));
        }
    }
    log::debug!("Remote version {:?}", max);
    write_cache(cache_dir, &Cache {
        timestamp,
        expires: timestamp +
            if max.is_some() { cache_age() } else { negative_cache_age() },
        version: max.cloned(),
    })?;
    Ok(())
}

fn cache_dir() -> anyhow::Result<PathBuf> {
    let dir = home_dir()?.join(".edgedb").join("cache");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn check(no_version_check_opt: bool) {
    if no_version_check_opt {
        log::debug!("Skipping version check due to --no-version-check");
        return;
    }
    if env::var_os("EDGEDB_NO_VERSION_CHECK")
        .map(|x| !x.is_empty()).unwrap_or(false)
    {
        log::debug!("Skipping version check due to EDGEDB_NO_VERSION_CHECK");
        return;
    }
    let dir = match cache_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::debug!("Version check ignored: {}", e);
            return;
        }
    };
    match _check(&dir) {
        Ok(()) => {}
        Err(e) => {
            log::warn!("Cannot check for updates: {}", e);
        }
    }
}
