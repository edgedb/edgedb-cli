use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};

use fn_error_context::context;
use rand::{thread_rng, Rng};
use serde::{Serialize, Deserialize};

use crate::platform;
use crate::server::version::Version;
use crate::self_upgrade;


#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    #[serde(with="humantime_serde")]
    timestamp: SystemTime,
    #[serde(with="humantime_serde")]
    expires: SystemTime,
    version: Option<Version<String>>,
}

fn cache_age() -> Duration {
    Duration::from_secs(thread_rng().gen_range(16*3600..32*3600))
}

fn negative_cache_age() -> Duration {
    Duration::from_secs(thread_rng().gen_range(6*3600..12*3600))
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

fn newer_warning(ver: &Version<String>) {
    if self_upgrade::can_upgrade() {
        log::warn!(
            "Newer version of edgedb tool exists {} (current {}). \
                To upgrade run `edgedb self upgrade`",
            ver, env!("CARGO_PKG_VERSION"));
    } else {
        log::warn!(
            "Newer version of edgedb tool exists {} (current {})",
            ver, env!("CARGO_PKG_VERSION"));
    }
}

fn _check(cache_dir: &Path) -> anyhow::Result<()> {
    match read_cache(cache_dir) {
        Ok(cache) if cache.expires > SystemTime::now() => {
            log::debug!("Cached version {:?}", cache.version);
            if let Some(ver) = cache.version {
                if Version(env!("CARGO_PKG_VERSION").into()) < ver {
                    newer_warning(&ver);
                }
            }
            return Ok(());
        }
        Ok(_) => {},
        Err(e) => {
            log::debug!("Error reading cache: {}", e);
        }
    }
    let timestamp = SystemTime::now();
    let repo = match self_upgrade::get_repo(Duration::from_secs(1)) {
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
            newer_warning(&ver);
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
    let dir = platform::cache_dir()?;
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
