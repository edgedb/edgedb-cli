use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};

use fn_error_context::context;
use rand::{thread_rng, Rng};
use serde::{Serialize, Deserialize};

use crate::cli;
use crate::platform;
use crate::portable::repository;


#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    #[serde(with="humantime_serde")]
    timestamp: SystemTime,
    #[serde(with="humantime_serde")]
    expires: SystemTime,
    #[serde(with="serde_str::opt")]
    version: Option<semver::Version>,
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

fn newer_warning(ver: &semver::Version) {
    if cli::upgrade::can_upgrade() {
        log::warn!(
            "Newer version of edgedb tool exists {} (current {}). \
                To upgrade run `edgedb cli upgrade`",
            ver, env!("CARGO_PKG_VERSION"));
    } else {
        log::warn!(
            "Newer version of edgedb tool exists {} (current {})",
            ver, env!("CARGO_PKG_VERSION"));
    }
}

fn _check(cache_dir: &Path) -> anyhow::Result<()> {
    let self_version = cli::upgrade::self_version()?;
    match read_cache(cache_dir) {
        Ok(cache) if cache.expires > SystemTime::now() => {
            log::debug!("Cached version {:?}", cache.version);
            if let Some(ver) = cache.version {
                if self_version < ver {
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
    let pkg = repository::get_cli_packages(cli::upgrade::channel())?
        .into_iter()
        .map(|pkg| pkg.version)
        .max();
    if let Some(ver) = &pkg {
        if &self_version < ver {
            newer_warning(&ver);
        }
    }
    log::debug!("Remote version {:?}", pkg);
    write_cache(cache_dir, &Cache {
        timestamp,
        expires: timestamp +
            if pkg.is_some() { cache_age() } else { negative_cache_age() },
        version: pkg.clone(),
    })?;
    Ok(())
}

fn cache_dir() -> anyhow::Result<PathBuf> {
    let dir = platform::cache_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn check(no_version_check_opt: bool) -> anyhow::Result<()> {
    if no_version_check_opt {
        log::debug!("Skipping version check due to --no-cli-update-check");
        return Ok(());
    }
    match env::var("EDGEDB_RUN_VERSION_CHECK").as_ref().map(|x| &x[..]) {
        Ok("never") => {
            log::debug!(
                "Skipping version check due to \
                EDGEDB_RUN_VERSION_CHECK=never");
            return Ok(());
        }
        Ok("cached") | Ok("default") => {}
        Ok(value) => {
            anyhow::bail!("unexpected value of EDGEDB_RUN_VERSION_CHECK: {:?} \
                           Options: never, cached, default.",
                          value);
        }
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(value)) => {
            anyhow::bail!("unexpected value of EDGEDB_RUN_VERSION_CHECK: {:?} \
                           Options: never, cached, default.",
                          value);
        }
    }
    let dir = match cache_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::debug!("Version check ignored: {}", e);
            return Ok(());
        }
    };
    match _check(&dir) {
        Ok(()) => {}
        Err(e) => {
            log::warn!("Cannot check for updates: {}", e);
        }
    }
    Ok(())
}
