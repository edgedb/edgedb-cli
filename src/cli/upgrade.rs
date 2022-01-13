use std::env;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_std::task;
use edgedb_cli_derive::EdbClap;
use fn_error_context::context;
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};

use crate::platform::{home_dir, binary_path, tmp_file_path, current_exe};
use crate::print::{self, echo, Highlight};
use crate::process;
use crate::portable::ver;
use crate::portable::repository::{self, download, Channel};


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
    /// Upgrade to the latest nightly version
    #[clap(long)]
    pub to_nightly: bool,
    /// Upgrade to the latest stable version
    #[clap(long)]
    pub to_stable: bool,
}


pub fn can_upgrade() -> bool {
    binary_path().and_then(|p| _can_upgrade(&p)).unwrap_or_else(|e| {
        log::info!("Cannot compare current binary to default: {}", e);
        false
    })
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
    let exe_path = current_exe()?;
    Ok(exe_path == path ||
       matches!(old_binary_path(), Ok(old) if exe_path == old))
}

#[context("error unpacking {:?} -> {:?}", src, tgt)]
pub fn unpack_file(src: &Path, tgt: &Path,
               compression: Option<repository::Compression>)
    -> anyhow::Result<()>
{
    fs::remove_file(&tgt).ok();
    match compression {
        Some(repository::Compression::Zstd) => {
            fs::remove_file(&tgt).ok();
            let src_f = fs::File::open(&src)?;

            let mut opt = fs::OpenOptions::new();
            opt.write(true).create_new(true);
            #[cfg(unix)] {
                use fs_err::os::unix::fs::OpenOptionsExt;
                opt.mode(0o755);
            }
            let mut tgt_f = opt.open(&tgt)?;

            let bar = ProgressBar::new(src.metadata()?.len());
            bar.set_style(
                ProgressStyle::default_bar()
                .template("Unpacking [{bar}] {bytes:>7.dim}/{total_bytes:7}")
                .progress_chars("=> "));
            let mut decoded = zstd::Decoder::new(io::BufReader::new(
                bar.wrap_read(src_f)
            ))?;
            io::copy(&mut decoded, &mut tgt_f)?;
            fs::remove_file(&src).ok();
            Ok(())
        }
        None => {
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&src, PermissionsExt::from_mode(0o755))?;
            }
            fs::rename(&src, &tgt)?;
            Ok(())
        }
    }

}

pub fn channel() -> repository::Channel {
    if env!("CARGO_PKG_VERSION").contains("-dev.") {
        Channel::Nightly
    } else {
        Channel::Stable
    }
}

pub fn self_version() -> anyhow::Result<ver::Semver> {
    env!("CARGO_PKG_VERSION").parse()
        .context("cannot parse cli version")
}

pub fn main(options: &CliUpgrade) -> anyhow::Result<()> {
    let path = binary_path()?;
    if !_can_upgrade(&path)? {
        anyhow::bail!("Only binary installed at {:?} can be upgraded", path);
    }
    let cur_channel = channel();
    let channel = if options.to_stable {
        Channel::Stable
    } else if options.to_nightly {
        Channel::Nightly
    } else {
        cur_channel
    };

    let pkg = repository::get_cli_packages(channel)?
        .into_iter().max_by(|a, b| a.version.cmp(&b.version))
        .context("cannot find new version")?;
    // Always force upgrade when switching channel
    let force = options.force || cur_channel != channel;
    if !force && pkg.version <= self_version()? {
        log::info!("Version is the same. No update needed.");
        if !options.quiet {
            print::success("Already up to date.");
        }
        return Ok(());
    }

    let down_path = path.with_extension("download");
    let tmp_path = tmp_file_path(&path);
    task::block_on(download(&down_path, &pkg.url, options.quiet, true))?;
    unpack_file(&down_path, &tmp_path, pkg.compression)?;

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
    process::Native::new("upgrade", "cli", &tmp_path)
        .arg("cli").arg("install").arg("--upgrade")
        .no_proxy().run()?;
    fs::remove_file(&tmp_path).ok();
    if !options.quiet {
        echo!("Upgraded to version", pkg.version.emphasize());
    }
    Ok(())
}
