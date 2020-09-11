use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use fn_error_context::context;

use crate::platform::ProcessGuard;
use crate::platform::home_dir;
use crate::server::control::read_metadata;
use crate::server::control;
use crate::server::init::{self, read_ports, init_credentials, Storage};
use crate::server::is_valid_name;
use crate::server::linux;
use crate::server::macos;
use crate::server::metadata::Metadata;
use crate::server::options::{Start, StartConf};
use crate::server::os_trait::{Method};
use crate::server::package::Package;
use crate::server::status::{Service, Status, DataDirectory};
use crate::server::status::{read_upgrade, backup_status, probe_port};


pub fn bootstrap(method: &dyn Method, settings: &init::Settings)
    -> anyhow::Result<()>
{
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

    method.create_user_service(&settings).map_err(|e| {
        eprintln!("Bootrapping complete, \
            but there was an error creating a service. \
            You can run server manually via: \n  \
            edgedb server start --foreground {}",
            settings.name.escape_default());
        e
    }).context("failed to init service")?;
    match (settings.start_conf, settings.inhibit_start) {
        (StartConf::Auto, false) => {
            let inst = method.get_instance(&settings.name)?;
            inst.start(&Start {
                name: settings.name.clone(),
                foreground: false,
            })?;
            init_credentials(&settings, &inst)?;
            println!("Bootstrap complete. Server is up and runnning now.");
        }
        (StartConf::Manual, _) | (_, true) => {
            todo!();
            /*
            let inst = method.get_instance(&settings.name)?;
            let mut cmd = inst.run_command()?;
            log::debug!("Running server: {:?}", cmd);
            let child = ProcessGuard::run(&mut cmd)
                .with_context(||
                    format!("error running server {:?}", cmd))?;
            init_credentials(&settings, &inst)?;
            drop(child);
            println!("Bootstrap complete. To start a server:\n  \
                      edgedb server start {}",
                      settings.name.escape_default());
            */
        }
    }
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

pub fn status(name: &String, data_dir: &Path,
    service_exists: bool, service: Service)
    -> Status
{
    use DataDirectory::*;

    let base = data_dir.parent().expect("data dir is not root");

    let (data_status, metadata) = if data_dir.exists() {
        let metadata = read_metadata(&data_dir);
        if metadata.is_ok() {
            let upgrade_file = data_dir.join("UPGRADE_IN_PROGRESS");
            if upgrade_file.exists() {
                (Upgrading(read_upgrade(&upgrade_file)), metadata)
            } else {
                (Normal, metadata)
            }
        } else {
            (NoMetadata, metadata)
        }
    } else {
        (Absent, Err(anyhow::anyhow!("No data directory")))
    };
    let reserved_port =
        // TODO(tailhook) cache ports
        read_ports()
        .map_err(|e| log::warn!("{:#}", e))
        .ok()
        .and_then(|ports| ports.get(name).cloned());
    let port_status = probe_port(&metadata, &reserved_port);
    let backup = backup_status(&base.join(format!("{}.backup", name)));
    let credentials_file_exists = home_dir().map(|home| {
        home.join(".edgedb")
            .join("credentials")
            .join(format!("{}.json", name))
            .exists()
    }).unwrap_or(false);

    Status {
        name: name.into(),
        service,
        metadata,
        reserved_port,
        port_status,
        storage: Storage::UserDir(data_dir.into()),
        data_status,
        backup,
        service_exists,
        credentials_file_exists,
    }
}

pub fn base_data_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Can't determine data directory"))?
        .join("edgedb").join("data"))
}
