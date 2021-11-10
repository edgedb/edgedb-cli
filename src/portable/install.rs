use std::fs;
use std::io;
use std::path::{Path, PathBuf, Component};
use std::time::SystemTime;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use indicatif::{ProgressBar, ProgressStyle};

use crate::commands::ExitCode;
use crate::platform;
use crate::portable::exit_codes;
use crate::portable::local;
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{PackageInfo, PackageHash, Query};
use crate::portable::repository::{get_server_package, download};
use crate::portable::ver;
use crate::print::{self, eecho, Highlight};
use crate::server::options::Install;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstallInfo {
    pub version: ver::Build,
    pub package_url: url::Url,
    pub package_hash: PackageHash,
    #[serde(with="serde_millis")]
    pub installed_at: SystemTime,
}

#[context("metadata error for {:?}", dir)]
fn check_metadata(dir: &Path, pkg_info: &PackageInfo)
    -> anyhow::Result<InstallInfo>
{
    let data = local::read_metadata(dir)?;
    if data.version != pkg_info.version {
        log::warn!("Remote package has version of {},
                    installed package version: {}",
                    pkg_info.version, data.version);
    }
    log::info!("Package {} was installed at {}, location: {:?}",
               data.version, humantime::format_rfc3339(data.installed_at), dir);
    Ok(data)
}

#[context("failed to download {}", pkg_info)]
fn download_package(pkg_info: &PackageInfo)
    -> anyhow::Result<PathBuf>
{
    let cache_dir = platform::cache_dir()?;
    let download_dir = cache_dir.join("downloads");
    fs::create_dir_all(&download_dir)?;
    let cache_path = download_dir.join(pkg_info.cache_file_name());
    let hash = task::block_on(download(&cache_path, &pkg_info.url))?;
    match &pkg_info.hash {
        PackageHash::Blake2b(hex) => {
            if hash.to_hex()[..] != hex[..] {
                anyhow::bail!("hash mismatch {} != {}", hash.to_hex(), hex);
            }
        }
        PackageHash::Unknown(val) => {
            log::warn!("Cannot verify hash, unknown hash format {:?}", val);
        }
    }
    Ok(cache_path)
}

#[context("cannot write metadata {:?}", path)]
fn write_meta(path: &Path, data: &InstallInfo) -> anyhow::Result<()> {
    let file = io::BufWriter::new(fs::File::create(path)?);
    serde_json::to_writer_pretty(file, data)?;
    Ok(())
}

fn build_path(base: &Path, path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut components = path.components()
        .filter_map(|part| {
            match part {
                Component::Normal(part) => Some(Ok(part)),
                // Leading '/' characters, root paths, and '.'
                // components are just ignored and treated as "empty
                // components"
                Component::Prefix(..) | Component::RootDir | Component::CurDir
                    => None,
                // If any part of the filename is '..', then skip over
                // unpacking the file to prevent directory traversal
                // security issues.  See, e.g.: CVE-2001-1267,
                // CVE-2002-0399, CVE-2005-1918, CVE-2007-4131
                Component::ParentDir
                    => Some(Err(anyhow::anyhow!("erroneous path {:?}", path))),
            }
        });
    if let Some(directory_name) = components.next() {
        directory_name?;
    } else {
        return Ok(None); // skipping root
    }

    let mut dest = PathBuf::from(base);
    if let Some(component) = components.next() {
        dest.push(component?);
    } else {
        return Ok(None); // the package directory itself
    }
    for component in components {
        let component = component?;
        match dest.symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => {
                anyhow::bail!("cannot unpack {:?} to the symlinked dir {:?}",
                              path, dest);
            }
            Ok(m) if m.file_type().is_file() => {
                anyhow::bail!("{:?} is a file not a directory for {:?}",
                              dest, path);
            }
            Ok(_) => {}
            Err(_) => {
                fs::create_dir(&dest)?;
            }
        }
        dest.push(component);
    }
    Ok(Some(dest))
}

#[context("failed to unpack {:?} -> {:?}", cache_file, target_dir)]
fn unpack_package(cache_file: &Path, target_dir: &Path)
    -> anyhow::Result<()>
{
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;

    // needed for long paths on windows
    let target_dir = target_dir.canonicalize()?;

    let file = fs::File::open(&cache_file)?;
    let bar = ProgressBar::new(file.metadata()?.len());
    bar.set_style(
        ProgressStyle::default_bar()
        .template("Unpacking [{bar}] {bytes:>7.dim}/{total_bytes:7}")
        .progress_chars("=> "));
    let file = zstd::Decoder::new(io::BufReader::new(bar.wrap_read(file)))?;
    let mut arch = tar::Archive::new(file);

    for entry in arch.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if let Some(path) = build_path(&target_dir, &*path)? {
            entry.unpack(path)?;
        }
    }
    bar.finish_and_clear();
    Ok(())
}

fn unlink_cache(cache_file: &Path) {
    fs::remove_file(&cache_file)
        .map_err(|e| {
            log::warn!("Failed to remove cache {:?}: {}", cache_file, e);
        }).ok();
}

pub fn install(options: &Install) -> anyhow::Result<()> {
    if options.method.is_some() {
        return crate::server::install::install(options);
    }
    if optional_docker_check()? {
        print::error(
            "`edgedb server install` in a Docker container is not supported.",
        );
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let query = Query::from_options(options.nightly, &options.version)?;
    version(&query)?;
    Ok(())
}

pub fn version(query: &Query) -> anyhow::Result<InstallInfo> {
    let pkg_info = get_server_package(&query)?
        .context("no package matching your criteria found")?;
    package(&pkg_info)
}

pub fn package(pkg_info: &PackageInfo) -> anyhow::Result<InstallInfo> {
    let ver_name = pkg_info.version.specific().to_string();
    let target_dir = platform::portable_dir()?.join(&ver_name);
    if target_dir.exists() {
        let meta = check_metadata(&target_dir, &pkg_info)?;
        eecho!("Version", meta.version.emphasize(), "is already installed");
        return Ok(meta);
    }

    eecho!("Downloading package...");
    let cache_path = download_package(&pkg_info)?;
    let tmp_target = platform::tmp_file_path(&target_dir);
    unpack_package(&cache_path, &tmp_target)?;
    let info = InstallInfo {
        version: pkg_info.version.clone(),
        package_url: pkg_info.url.clone(),
        package_hash: pkg_info.hash.clone(),
        installed_at: SystemTime::now(),
    };
    write_meta(&tmp_target.join("install_info.json"), &info)?;
    fs::rename(&tmp_target, &target_dir).with_context(
        || format!("cannot rename {:?} -> {:?}", tmp_target, target_dir))?;
    unlink_cache(&cache_path);
    eecho!("Successfully installed", pkg_info.version.emphasize());

    Ok(info)
}

impl InstallInfo {
    pub fn base_path(&self) -> anyhow::Result<PathBuf> {
        Ok(platform::portable_dir()?.join(self.version.specific().to_string()))
    }
    pub fn server_path(&self) -> anyhow::Result<PathBuf> {
        Ok(self.base_path()?.join("bin").join("edgedb-server"))
    }
}
