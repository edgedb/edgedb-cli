use std::fs;
use std::io;
use std::path::{Path, PathBuf, Component};

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use indicatif::{ProgressBar, ProgressStyle};

use crate::commands::ExitCode;
use crate::platform;
use crate::portable::exit_codes;
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{Channel, PackageInfo, download};
use crate::portable::repository::{get_server_package};
use crate::print;
use crate::server::options::Install;


#[context("failed to download {}", pkg_info)]
fn download_package(pkg_info: &PackageInfo) -> anyhow::Result<PathBuf> {
    let cache_dir = platform::cache_dir()?;
    let download_dir = cache_dir.join("downloads");
    fs::create_dir_all(&download_dir)?;
    let cache_path = download_dir.join(pkg_info.cache_file_name());
    task::block_on(download(&cache_path, &pkg_info.package_url))?;
    Ok(cache_path)
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
        eprintln!("\
            To obtain a Docker image with EdgeDB server installed, \
            run the following on the host system instead:\n  \
            edgedb server install --method=docker");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let channel = if options.nightly {
        Channel::Nightly
    } else {
        Channel::Stable
    };
    let ver_query = options.version.as_ref().map(|x| x.num().parse())
        .transpose().context("Unexpected --version")?;
    let pkg_info = get_server_package(channel, &ver_query)?
        .context("no package matching your criteria found")?;

    let cache_path = download_package(&pkg_info)?;
    let ver_name = pkg_info.version.specific().to_string();
    let target_dir = platform::portable_dir()?.join(&ver_name);
    let tmp_target = platform::tmp_file_path(&target_dir);
    unpack_package(&cache_path, &tmp_target)?;
    fs::rename(&tmp_target, &target_dir).with_context(
        || format!("cannot rename {:?} -> {:?}", tmp_target, target_dir))?;
    unlink_cache(&cache_path);
    Ok(())
}
