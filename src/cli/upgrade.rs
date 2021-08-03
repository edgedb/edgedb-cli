use std::process::Command;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration};

use anyhow::Context;
use async_std::task;
use edgedb_cli_derive::EdbClap;
use fn_error_context::context;
use indicatif::{ProgressBar, ProgressStyle};
use url::Url;

use crate::async_util::timeout;
use crate::platform::home_dir;
use crate::process;
use crate::server::package::RepositoryInfo;
use crate::server::remote;
use crate::server::version::Version;


#[derive(EdbClap, Clone, Debug)]
pub struct CliUpgrade {
    /// Enable verbose output
    #[clap(short='v', long)]
    pub verbose: bool,
    /// Disable progress output
    #[clap(short='q', long)]
    pub quiet: bool,
    /// Reinstall even if there is no newer version
    #[clap(long)]
    pub force: bool,
}


pub fn get_repo(max_wait: Duration) -> anyhow::Result<RepositoryInfo> {
    let platform =
        if cfg!(windows) {
            "win"
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

    task::block_on(timeout(
        max_wait,
        remote::get_json(&url, "cannot get package index for CLI tools"),
    ))
}

pub fn can_upgrade() -> bool {
    binary_path().and_then(|p| _can_upgrade(&p)).unwrap_or_else(|e| {
        log::info!("Cannot compare current binary to default: {}", e);
        false
    })
}

pub fn binary_path() -> anyhow::Result<PathBuf> {
    let dir = match dirs::executable_dir() {
        Some(dir) => dir,
        // windows and macos fit this branch
        None => {
            dirs::data_dir()
                .context("cannot determine local data directory")?
                .join("edgedb")
                .join("bin")
        }
    };
    let path = if cfg!(windows) {
        dir.join("edgedb.exe")
    } else {
        dir.join("edgedb")
    };
    Ok(path)
}

pub fn old_binary_path() -> anyhow::Result<PathBuf> {
    let bin_name = if cfg!(windows) {
        "edgedb.exe"
    } else {
        "edgedb"
    };
    Ok(home_dir()?.join(".edgedb").join("bin").join(bin_name))
}

fn _can_upgrade(path: &Path) -> anyhow::Result<bool> {
    let exe_path = env::current_exe()
        .with_context(|| format!("cannot determine running executable path"))?;
    Ok(exe_path == path ||
       matches!(old_binary_path(), Ok(old) if exe_path == old))
}

#[context("cannot download {} -> {}", url, path.display())]
async fn download(url: &str, path: &Path, quiet: bool) -> anyhow::Result<()> {
    use async_std::fs;
    use async_std::prelude::*;

    fs::remove_file(&path).await.ok();
    let mut opt = fs::OpenOptions::new();
    opt.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        opt.mode(0o777);
    }
    let mut out = opt.open(path).await?;
    let mut body = surf::get(url)
        .header("User-Agent", remote::USER_AGENT)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?
        .take_body();
    let bar = if quiet {
        ProgressBar::hidden()
    } else if let Some(len) = body.len() {
        ProgressBar::new(len as u64)
    } else {
        ProgressBar::new_spinner()
    };
    bar.set_style(
        ProgressStyle::default_bar()
        .template(
            "[{elapsed_precise}] {wide_bar} \
            {bytes:>7}/{total_bytes:7} | ETA: {eta}"));
    let mut buf = [0u8; 16384];
    loop {
        let bytes = body.read(&mut buf).await?;
        if bytes == 0 {
            break;
        }
        out.write_all(&buf[..bytes]).await?;
        bar.inc(bytes as u64);
    }
    Ok(())
}

pub fn main(options: &CliUpgrade) -> anyhow::Result<()> {
    let path = binary_path()?;
    if !_can_upgrade(&path)? {
        anyhow::bail!("Only binary installed at {:?} can be upgraded", path);
    }
    let repo = get_repo(Duration::from_secs(120))?;

    let max = repo.packages.iter()
        .filter(|pkg| pkg.basename == "edgedb-cli")
        .max_by_key(|pkg| (&pkg.version, &pkg.revision));
    let pkg = max.ok_or_else(|| anyhow::anyhow!("cannot find new version"))?;
    if !options.force &&
        pkg.version <= Version(env!("CARGO_PKG_VERSION").into())
    {
        log::info!("Version is the same. No update needed.");
        return Ok(());
    }

    let url = Url::parse("https://packages.edgedb.com/")
        .expect("hardcoded URL is valid")
        .join(&pkg.installref)
        .context("package installref is invalid")?;
    let tmp_path = path.with_extension("download");
    task::block_on(download(&url.to_string(), &tmp_path, options.quiet))?;
    let backup_path = path.with_extension("backup");
    if cfg!(unix) {
        fs::remove_file(&backup_path).ok();
        fs::hard_link(&path, &backup_path)
            .map_err(|e| log::warn!("Cannot keep a backup file: {:#}", e))
            .ok();
    } else if cfg!(windows) {
        fs::remove_file(&backup_path).ok();
        fs::rename(&path, &backup_path)?;
    } else {
        anyhow::bail!("unknown OS");
    }
    process::run(Command::new(&tmp_path)
        .arg("cli").arg("install").arg("--upgrade"))?;
    fs::remove_file(&tmp_path).ok();
    if !options.quiet {
        println!("Upgraded to version {} (revision {})",
            pkg.version, pkg.revision);
    }
    Ok(())
}
