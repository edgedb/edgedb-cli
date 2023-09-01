use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use edgedb_cli_derive::EdbClap;
use fn_error_context::context;
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};

use crate::platform::{home_dir, binary_path, tmp_file_path, current_exe};
use crate::portable::platform;
use crate::portable::repository::{self, download, Channel};
use crate::portable::ver;
use crate::print::{self, echo, Highlight};
use crate::process;


const INDEX_TIMEOUT: Duration = Duration::new(60, 0);


#[derive(EdbClap, Clone, Debug)]
pub struct CliUpgrade {
    /// Enable verbose output
    #[clap(short='v', long)]
    pub verbose: bool,
    /// Disable progress output
    #[clap(short='q', long)]
    pub quiet: bool,
    /// Force reinstall even if no newer version exists
    #[clap(long)]
    pub force: bool,
    /// Upgrade to latest nightly version
    #[clap(long)]
    #[clap(conflicts_with_all=&["to_testing", "to_stable", "to_channel"])]
    pub to_nightly: bool,
    /// Upgrade to latest stable version
    #[clap(long)]
    #[clap(conflicts_with_all=&["to_testing", "to_nightly", "to_channel"])]
    pub to_stable: bool,
    /// Upgrade to latest testing version
    #[clap(long)]
    #[clap(conflicts_with_all=&["to_stable", "to_nightly", "to_channel"])]
    pub to_testing: bool,
    /// Upgrade specified instance to specified channel
    #[clap(long, value_enum)]
    #[clap(conflicts_with_all=&["to_stable", "to_nightly", "to_testing"])]
    pub to_channel: Option<Channel>,
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
                .expect("template is ok")
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

pub fn channel_of(ver: &str) -> repository::Channel {
    if ver.contains("-dev.") {
        Channel::Nightly
    } else if ver.contains("-") {
        Channel::Testing
    } else {
        Channel::Stable
    }
}

pub fn channel() -> repository::Channel {
    channel_of(env!("CARGO_PKG_VERSION"))
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
    _main(options, path)
}

pub fn upgrade_to_arm64() -> anyhow::Result<()> {
    _main(&CliUpgrade {
        verbose: false,
        quiet: false,
        force: true,
        to_nightly: false,
        to_stable: false,
        to_testing: false,
        to_channel: None,
    }, binary_path()?)
}

fn _main(options: &CliUpgrade, path: PathBuf) -> anyhow::Result<()> {
    let cur_channel = channel();
    let channel = if let Some(channel) = options.to_channel {
        channel
    } else if options.to_stable {
        Channel::Stable
    } else if options.to_nightly {
        Channel::Nightly
    } else if options.to_testing {
        Channel::Testing
    } else {
        cur_channel
    };

    #[allow(unused_mut)]
    let mut target_plat = platform::get_cli()?;
    // Always force upgrade when switching channel
    #[allow(unused_mut)]
    let mut force = options.force || cur_channel != channel;

    if cfg!(all(target_os="macos", target_arch="x86_64")) &&
        platform::is_arm64_hardware()
    {
        target_plat = "aarch64-apple-darwin";
        // Always force upgrade when need to switch platform
        force = true;
    }

    let pkg = repository::get_platform_cli_packages(channel, target_plat,
                                                    INDEX_TIMEOUT)?
        .into_iter().max_by(|a, b| a.version.cmp(&b.version))
        .context("cannot find new version")?;
    if !force && pkg.version <= self_version()? {
        log::info!("Version is identical; no update needed.");
        if !options.quiet {
            print::success("Already up to date.");
        }
        return Ok(());
    }

    let down_path = path.with_extension("download");
    let tmp_path = tmp_file_path(&path);
    download(&down_path, &pkg.url, options.quiet)?;
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
