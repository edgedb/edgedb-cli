use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use fn_error_context::context;

use crate::server::init::{self, Storage};
use crate::server::metadata::Metadata;
use crate::server::linux;
use crate::server::macos;
use crate::server::package::Package;
use crate::server::is_valid_name;


pub fn bootstrap(settings: &init::Settings) -> anyhow::Result<()> {
    let dir = match &settings.storage {
        Storage::UserDir(path) => path,
        other => anyhow::bail!("unsupported storage {:?}", other),
    };
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create {}", dir.display()))?;

    let pkg = settings.distribution.downcast_ref::<Package>()
        .context("invalid unix package")?;
    let mut cmd = Command::new(if cfg!(target_os="macos") {
        macos::get_server_path(&pkg.slot)
    } else {
        linux::get_server_path(Some(&pkg.slot))
    });
    cmd.arg("--bootstrap");
    cmd.arg("--log-level=warn");
    cmd.arg("--data-dir").arg(&dir);
    if settings.inhibit_user_creation {
        cmd.arg("--default-database=edgedb");
        cmd.arg("--default-database-user=edgedb");
    }

    log::debug!("Running bootstrap {:?}", cmd);
    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
        Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
    }

    if let Some(upgrade_marker) = &settings.upgrade_marker {
        write_upgrade(
            &dir.join("UPGRADE_IN_PROGRESS"),
            upgrade_marker)?;
    }

    let metapath = dir.join("metadata.json");
    write_metadata(&metapath, &settings.metadata())?;
    Ok(())
}

#[context("failed to write upgrade marker {}", path.display())]
fn write_upgrade(path: &Path, data: &str) -> anyhow::Result<()> {
    fs::write(path, data.as_bytes())?;
    Ok(())
}

#[context("failed to write metadata file {}", path.display())]
fn write_metadata(path: &Path, metadata: &Metadata) -> anyhow::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(&metadata)?)?;
    Ok(())
}

pub fn storage(system: bool, name: &str) -> anyhow::Result<Storage> {
    Ok(Storage::UserDir(dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Can't determine data directory"))?
        .join("edgedb").join("data").join(name)))
}

pub fn clean_storage(storage: &Storage) -> anyhow::Result<()> {
    match storage {
        Storage::UserDir(path) => Ok(fs::remove_dir_all(&path)?),
        _ => anyhow::bail!("Storage {} is unsupported", storage.display()),
    }
}

pub fn storage_exists(storage: &Storage) -> anyhow::Result<bool> {
    match storage {
        Storage::UserDir(path) => Ok(path.exists()),
        _ => anyhow::bail!("Storage {} is unsupported", storage.display()),
    }
}

#[context("error reading dir {}", dir.display())]
pub fn instances_from_data_dir(dir: &Path, system: bool,
    instances: &mut BTreeSet<(String, bool)>)
    -> anyhow::Result<()>
{
    for item in fs::read_dir(&dir)? {
        let item = item?;
        if !item.file_type()?.is_dir() {
            continue;
        }
        if let Some(name) = item.file_name().to_str() {
            if !is_valid_name(name) {
                continue;
            }
            instances.insert((name.to_owned(), system));
        }
    }
    Ok(())
}
