use std::collections::{BTreeSet, BTreeMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::time::SystemTime;

use anyhow::Context;
use async_std::task;
use serde::Serialize;
use linked_hash_map::LinkedHashMap;
use fn_error_context::context;

use crate::commands::ExitCode;
use crate::process::ProcessGuard;
use crate::platform::{Uid, home_dir, get_current_uid};
use crate::server::control::read_metadata;
use crate::server::detect::{VersionQuery, Lazy};
use crate::server::errors::{CannotCreateService, CannotStartService};
use crate::server::init::{self, read_ports, init_credentials, Storage};
use crate::server::install::{self, exit_codes, Operation, operation};
use crate::server::is_valid_name;
use crate::server::linux;
use crate::server::macos;
use crate::server::metadata::Metadata;
use crate::server::options::{self, Start, Upgrade, StartConf, Stop};
use crate::server::os_trait::{Method, Instance, InstanceRef};
use crate::server::package::Package;
use crate::server::status::{Service, Status, DataDirectory};
use crate::server::status::{read_upgrade, backup_status, probe_port};
use crate::server::upgrade;
use crate::server::version::Version;


#[derive(Debug, Serialize)]
pub struct Unix {
    user_id: Lazy<Uid>,
    sudo_path: Lazy<Option<PathBuf>>,
}

impl Unix {
    pub fn new() -> Unix {
        Unix {
            user_id: Lazy::lazy(),
            sudo_path: Lazy::lazy(),
        }
    }
    pub fn detect_all(&self) {
        self.get_user_id();
        self.get_sudo_path();
    }
    pub fn get_user_id(&self) -> Uid {
        *self.user_id.get_or_init(|| {
            get_current_uid()
        })
    }
    pub fn get_sudo_path(&self) -> Option<&PathBuf> {
        self.sudo_path.get_or_init(|| {
            which::which("sudo").ok()
        }).as_ref()
    }
}

impl Unix {
    pub fn perform(&self, operations: Vec<Operation>,
        operation_name: &str, hint_cmd: &str)
        -> anyhow::Result<()>
    {
        let mut ctx = operation::Context::new();
        let has_privileged = operations.iter().any(|x| x.is_privileged());
        if has_privileged && self.get_user_id() != 0 {
            println!("The following commands will be run with elevated \
                privileges using sudo:");
            for op in &operations {
                if op.is_privileged() {
                    println!("    {}", op.format(true));
                }
            }
            println!("Depending on system settings sudo may now ask \
                      you for your password...");
            match self.get_sudo_path() {
                Some(cmd) => ctx.set_elevation_cmd(cmd),
                None => {
                    eprintln!("`sudo` command not found. \
                               Cannot elevate privileges needed for \
                               {}. Please run `{}` as root user.",
                               operation_name, hint_cmd);
                    return Err(ExitCode::new(exit_codes::NO_SUDO))?;
                }
            }
        }
        for op in &operations {
            op.perform(&ctx)?;
        }
        Ok(())
    }
}


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
    cmd.arg("--bootstrap-only");
    cmd.arg("--log-level=warn");
    cmd.arg("--data-dir").arg(&dir);

    log::debug!("Running bootstrap {:?}", cmd);
    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
        Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
    }

    let metapath = dir.join("metadata.json");
    let metadata = settings.metadata();
    write_metadata(&metapath, &metadata)?;

    let res = create_user_service(&settings.name, &metadata);

    match settings.start_conf {
        StartConf::Auto if res.is_ok() => {
            let inst = method.get_instance(&settings.name)?;
            inst.start(&Start {
                name: settings.name.clone(),
                foreground: false,
            })?;
            init_credentials(&settings, &inst)?;
            println!("Bootstrap complete. Server is up and running now.");
            println!("To connect run:\n  edgedb -I{}",
                     settings.name.escape_default());
        }
        _ => {
            let inst = method.get_instance(&settings.name)?;
            let mut cmd = inst.get_command()?;
            log::debug!("Running server: {:?}", cmd);
            let child = ProcessGuard::run(&mut cmd)
                .with_context(||
                    format!("error running server {:?}", cmd))?;
            init_credentials(&settings, &inst)?;
            drop(child);
            if settings.start_conf == StartConf::Manual && res.is_ok() {
                println!("Bootstrap complete. To start the server:\n  \
                          edgedb server start {}",
                          settings.name.escape_default());
            }
        }
    }
    res.map_err(CannotCreateService)?;
    Ok(())
}

fn create_user_service(name: &str, meta: &Metadata) -> anyhow::Result<()> {
    if cfg!(target_os="macos") {
        macos::create_launchctl_service(&name, &meta)
    } else if cfg!(target_os="linux") {
        linux::create_systemd_service(&name, &meta)
    } else {
        anyhow::bail!("unsupported OS");
    }
}

#[context("failed to write upgrade marker {}", path.display())]
fn write_upgrade(path: &Path, data: &str) -> anyhow::Result<()> {
    fs::write(path, data.as_bytes())?;
    Ok(())
}

#[context("failed to write metadata file {}", path.display())]
fn write_metadata(path: &Path, metadata: &Metadata) -> anyhow::Result<()> {
    let tmp_path = path.with_extension("tmp");
    fs::remove_file(&tmp_path).ok();
    fs::write(&tmp_path, serde_json::to_vec_pretty(&metadata)?)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn storage_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("Can't determine data directory"))?
        .join("edgedb").join("data").join(name))
}

pub fn storage(system: bool, name: &str) -> anyhow::Result<Storage> {
    assert!(!system);
    Ok(Storage::UserDir(storage_dir(name)?))
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
        let upgrade_file = base
            .join(format!("{}.UPGRADE_IN_PROGRESS", name));
        if upgrade_file.exists() {
            (Upgrading(read_upgrade(&upgrade_file)), metadata)
        } else {
            if metadata.is_ok() {
                (Normal, metadata)
            } else {
                (NoMetadata, metadata)
            }
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

pub fn upgrade(todo: &upgrade::ToDo, options: &Upgrade, meth: &dyn Method)
    -> anyhow::Result<()>
{
    use upgrade::ToDo::*;

    match todo {
        MinorUpgrade => do_minor_upgrade(meth, options),
        NightlyUpgrade => do_nightly_upgrade(meth, options),
        InstanceUpgrade(name, ref version) => {
            let inst = meth.get_instance(name)?;
            do_instance_upgrade(meth, inst, version, options)
        }
    }
}

fn do_minor_upgrade(method: &dyn Method, options: &Upgrade)
    -> anyhow::Result<()>
{
    let mut by_major = BTreeMap::new();
    for inst in method.all_instances()? {
        if inst.get_version()?.is_nightly() {
            continue;
        }
        by_major.entry(inst.get_version()?.clone())
            .or_insert_with(Vec::new)
            .push(inst);
    }
    let mut errors = Vec::new();
    for (version, mut instances) in by_major {
        let instances_str = instances
            .iter().map(|inst| inst.name()).collect::<Vec<_>>().join(", ");

        let version_query = version.to_query();
        let new = method.get_version(&version_query)
            .context("Unable to determine version")?;
        let old = upgrade::get_installed(&version_query, method)?;
        let new_major = new.major_version().clone();
        let new_version = new.version().clone();
        let slot = new.downcast_ref::<Package>()
            .context("invalid linux package")?
            .slot.clone();

        if !options.force {
            if let Some(old_ver) = &old {
                if old_ver >= new.version() {
                    log::info!(target: "edgedb::server::upgrade",
                        "Version {} is up to date {}, skipping instances: {}",
                        version.title(), old_ver, instances_str);
                    return Ok(());
                }
            }
        }

        log::info!("Upgrading version: {} to {}, instances: {}",
            version.title(), new.version(), instances_str);

        // Stop instances first.
        //
        // This (launchctl unload) is required for MacOS to reinstall
        // the pacakge. On other systems, this is also useful as in-place
        // modifying the running package isn't very good idea.
        for inst in &mut instances {
            inst.stop(&options::Stop { name: inst.name().into() })
                .map_err(|e| {
                    log::warn!("Failed to stop instance {:?}: {:#}",
                        inst.name(), e);
                })
                .ok();
        }

        log::info!(target: "edgedb::server::upgrade", "Upgrading the package");
        method.install(&install::Settings {
            method: method.name(),
            distribution: new,
            extra: LinkedHashMap::new(),
        })?;

        for inst in &instances {
            let new_meta = Metadata {
                version: new_major.clone(),
                slot: Some(slot.clone()),
                current_version: Some(new_version.clone()),
                method: method.name(),
                port: inst.get_port()?,
                start_conf: inst.get_start_conf()?,
            };
            let metapath = storage_dir(inst.name())?.join("metadata.json");
            write_metadata(&metapath, &new_meta)?;
            if inst.get_start_conf()? == StartConf::Auto {
                inst.start(&options::Start {
                        name: inst.name().into(),
                        foreground: false,
                    })
                    .with_context(|| format!("failed to start instance {:?}",
                                             inst.name()))
                    .map_err(|e| errors.push(format!("{:#}", e)))
                    .ok();
            }
        }
    }
    print_errors(errors)?;
    Ok(())
}

fn print_errors(errors: Vec<String>) -> anyhow::Result<()> {
    if errors.is_empty() {
        return Ok(());
    }
    eprintln!("Upgrade complete, but cannot start instances:");
    for er in errors {
        eprintln!("  {}", er);
    }
    return Err(ExitCode::new(2))?;
}

fn do_nightly_upgrade(method: &dyn Method, options: &Upgrade)
    -> anyhow::Result<()>
{
    let mut instances = Vec::new();
    for inst in method.all_instances()? {
        if !inst.get_version()?.is_nightly() {
            continue;
        }
        instances.push(inst);
    }
    if instances.is_empty() {
        return Ok(());
    }

    let version_query = VersionQuery::Nightly;
    let new = method.get_version(&version_query)
        .context("Unable to determine version")?;
    let slot = new.downcast_ref::<Package>()
        .context("invalid linux package")?
        .slot.clone();

    log::info!(target: "edgedb::server::upgrade",
        "Installing nightly {}", new.version());
    let new_version = new.version().clone();
    let new_major = new.major_version().clone();
    method.install(&install::Settings {
        method: method.name(),
        distribution: new,
        extra: LinkedHashMap::new(),
    })?;

    let mut errors = Vec::new();

    for inst in instances {
        let old = inst.get_current_version()?;

        if !options.force {
            if let Some(old_ver) = old {
                if old_ver >= &new_version {
                    log::info!(target: "edgedb::server::upgrade",
                        "Instance {} is up to date {}. Skipping.",
                        inst.name(), old_ver);
                    return Ok(());
                }
            }
        }
        let dump_path = storage_dir(inst.name())?
            .parent().expect("instance path can't be root")
            .join(format!("{}.dump", inst.name()));
        let new_meta = Metadata {
            version: new_major.clone(),
            slot: Some(slot.clone()),
            current_version: Some(new_version.clone()),
            method: method.name(),
            port: inst.get_port()?,
            start_conf: inst.get_start_conf()?,
        };
        upgrade::dump_and_stop(inst.as_ref(), &dump_path)?;
        let upgrade_meta = upgrade::UpgradeMeta {
            source: old.cloned().unwrap_or_else(|| Version("unknown".into())),
            target: new_version.clone(),
            started: SystemTime::now(),
            pid: process::id(),
        };
        let inst = inst.upgrade(&new_meta)?;
        match reinit_and_restore(inst.as_ref(), &new_meta, &upgrade_meta) {
            Ok(()) => {}
            Err(e) if e.is::<CannotStartService>() => {
                errors.push(
                    format!("failed to start instance {:?}: {:#}",
                        inst.name(), e));
            }
            Err(e) => return Err(e)?,
        }
    }
    print_errors(errors)?;
    Ok(())
}

#[context("failed to restore {:?}", inst.name())]
fn reinit_and_restore(inst: &dyn Instance, new_meta: &Metadata,
    upgrade_meta: &upgrade::UpgradeMeta)
    -> anyhow::Result<()>
{
    let instance_dir = storage_dir(inst.name())?;
    let base = instance_dir.parent().expect("instancedir is not root");

    let upgrade_marker = base
        .join(format!("{}.UPGRADE_IN_PROGRESS", inst.name()));
    if upgrade_marker.exists() {
        anyhow::bail!("Upgrade is already in progress");
    }
    write_upgrade(&upgrade_marker,
        &serde_json::to_string(&upgrade_meta).unwrap())?;

    let backup = base.join(&format!("{}.backup", inst.name()));
    if backup.exists() {
        fs::remove_dir_all(&backup)?;
    }
    fs::rename(&instance_dir, &backup)?;
    upgrade::write_backup_meta(&backup.join("backup.json"),
        &upgrade::BackupMeta {
            timestamp: SystemTime::now(),
        })?;
    _reinit_and_restore(
        &instance_dir, inst, new_meta, &upgrade_marker
    ).map_err(|e| {
        eprintln!("edgedb error: failed to restore {:?}: {}", inst.name(), e);
        eprintln!("To undo run:\n  edgedb server revert {:?}", inst.name());
        ExitCode::new(1).into()
    })
}

fn _reinit_and_restore(instance_dir: &Path, inst: &dyn Instance,
    new_meta: &Metadata, upgrade_marker: &Path)
    -> anyhow::Result<()>
{

    fs::create_dir_all(&instance_dir)
        .with_context(|| {
            format!("failed to create {}", instance_dir.display())
        })?;

    let mut cmd = inst.get_command()?;
    log::debug!("Running server: {:?}", cmd);
    let child = ProcessGuard::run(&mut cmd)
        .with_context(|| format!("error running server {:?}", cmd))?;

    let dump_path = storage_dir(inst.name())?
        .parent().expect("instance path can't be root")
        .join(format!("{}.dump", inst.name()));
    task::block_on(
        upgrade::restore_instance(inst, &dump_path, inst.get_connector(true)?)
    )?;
    log::info!(target: "edgedb::server::upgrade",
        "Restarting instance {:?} to apply changes from `restore --all`",
        &inst.name());
    drop(child);

    let metapath = instance_dir.join("metadata.json");
    write_metadata(&metapath, &new_meta)?;

    let res = create_user_service(inst.name(), &new_meta)
        .map_err(CannotStartService);

    fs::remove_file(&upgrade_marker)
        .with_context(|| format!("removing {:?}", upgrade_marker.display()))?;

    if inst.get_start_conf()? == StartConf::Auto {
        res?;
        inst.start(&Start { name: inst.name().into(), foreground: false })
            .map_err(CannotStartService)?;
    } else {
        res.map_err(|e| {
            log::warn!("Could not update service file. \
                Only `start --foreground` is supported. Error: {:#}",
                    anyhow::anyhow!(e));
        }).ok();
    }
    Ok(())
}

fn do_instance_upgrade(method: &dyn Method,
    inst: InstanceRef, version_query: &Option<VersionQuery>, options: &Upgrade)
    -> anyhow::Result<()>
{
    let version_query = if let Some(q) = version_query {
        q
    } else if inst.get_version()?.is_nightly() {
        &VersionQuery::Nightly
    } else {
        &VersionQuery::Stable(None)
    };
    let old = inst.get_current_version()?;
    let new = method.get_version(&version_query)
        .context("Unable to determine version")?;

    let new_major = new.major_version().clone();
    let old_major = inst.get_version()?;
    let new_version = new.version().clone();
    if !options.force {
        if &new_major == old_major && !new_major.is_nightly() {
            log::warn!(target: "edgedb::server::upgrade",
                "Instance has up to date major version {}. \
                 Use `edgedb server upgrade` (without instance name) \
                 to upgrade minor versions.",
                 old.map_or_else(|| old_major.title(), |v| v.num()));
            return Ok(());
        }
        if let Some(old_ver) = &old {
            if old_ver >= &new.version() {
                log::info!(target: "edgedb::server::upgrade",
                    "Version {} is up to date {}, skipping instance: {}",
                    version_query, old_ver, inst.name());
                return Ok(());
            }
        }
    }
    let slot = new.downcast_ref::<Package>()
        .context("invalid linux package")?
        .slot.clone();

    log::info!(target: "edgedb::server::upgrade", "Installing the package");
    method.install(&install::Settings {
        method: method.name(),
        distribution: new,
        extra: LinkedHashMap::new(),
    })?;

    let dump_path = storage_dir(inst.name())?
        .parent().expect("instance path can't be root")
        .join(format!("{}.dump", inst.name()));
    let new_meta = Metadata {
        version: new_major,
        slot: Some(slot),
        current_version: Some(new_version.clone()),
        method: method.name(),
        port: inst.get_port()?,
        start_conf: inst.get_start_conf()?,
    };
    upgrade::dump_and_stop(inst.as_ref(), &dump_path)?;


    let upgrade_meta = upgrade::UpgradeMeta {
        source: old.cloned().unwrap_or_else(|| Version("unknown".into())),
        target: new_version,
        started: SystemTime::now(),
        pid: process::id(),
    };
    let inst = inst.upgrade(&new_meta)?;
    match reinit_and_restore(inst.as_ref(), &new_meta, &upgrade_meta) {
        Ok(()) => {}
        Err(e) if e.is::<CannotStartService>() => {
            eprintln!("Upgrade complete, but cannot start instance: {:#}", e);
            return Err(ExitCode::new(2))?;
        }
        Err(e) => return Err(e)?,
    }

    Ok(())
}

pub fn revert(instance: &dyn Instance, metadata: &Metadata)
    -> anyhow::Result<()>
{
    let name = instance.name();
    instance.stop(&Stop { name: name.into() })?;
    let dest = storage_dir(&name)?;
    let base = dest.parent().expect("instance dir is not root");
    let backup = base.join(format!("{}.backup", name));
    let upgrading = base.join(format!("{}.UPGRADE_IN_PROGRESS", name));
    fs::remove_file(&upgrading).map_err(|e| {
        log::info!("Error deleting {:?}: {:#}", upgrading, e)
    }).ok();
    fs::remove_dir_all(&dest)
        .with_context(|| format!("deleting {:?}", dest))?;
    fs::rename(&backup, &dest)
        .with_context(|| format!("renaming {:?} -> {:?}", backup, dest))?;

    let res = create_user_service(name, &metadata)
        .map_err(CannotStartService);

    let backup_json = base.join("backup.json");
    fs::remove_file(&backup_json).map_err(|e| {
        log::info!("Error deleting {:?}: {:#}", backup_json, e)
    }).ok();

    if instance.get_start_conf()? == StartConf::Auto {
        res?;
        instance.start(&Start { name: name.into(), foreground: false })
            .map_err(CannotStartService)?;
    } else {
        res.map_err(|e| {
            log::warn!("Could not update service file. \
                Only `start --foreground` is supported. Error: {:#}",
                    anyhow::anyhow!(e));
        }).ok();
    }
    Ok(())
}
