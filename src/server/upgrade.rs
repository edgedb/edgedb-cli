use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::collections::BTreeMap;
use std::time::{SystemTime, Duration};

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use linked_hash_map::LinkedHashMap;
use serde::{Serialize, Deserialize};

use edgedb_client as client;
use crate::server::control;
use crate::server::detect::{self, VersionQuery};
use crate::server::init::{init};
use crate::server::install;
use crate::server::metadata::Metadata;
use crate::server::methods::Methods;
use crate::server::options::{self, Upgrade};
use crate::server::os_trait::{Method, InstanceRef};
use crate::server::version::Version;
use crate::server::is_valid_name;
use crate::server::distribution::MajorVersion;
use crate::commands;
use crate::platform::ProcessGuard;


#[derive(Serialize, Deserialize, Debug)]
pub struct UpgradeMeta {
    pub source: Version<String>,
    pub target: Version<String>,
    #[serde(with="humantime_serde")]
    pub started: SystemTime,
    pub pid: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BackupMeta {
    #[serde(with="humantime_serde")]
    pub timestamp: SystemTime,
}

struct Instance<'a> {
    instance: InstanceRef<'a>,
    source: Option<Version<String>>,
    version: Option<Version<String>>,
}

pub enum ToDo {
    MinorUpgrade,
    InstanceUpgrade(String, VersionQuery),
    NightlyUpgrade,
}

struct InstanceIterator {
    dir: fs::ReadDir,
    path: PathBuf,
}


fn interpret_options(options: &Upgrade) -> ToDo {
    if let Some(name) = &options.name {
        if options.nightly {
            eprintln!("Cannot upgrade specific nightly instance, \
                use `--to-nightly` to upgrade to nightly. \
                Use `--nightly` without instance name to upgrade all nightly \
                instances");
        }
        let nver = if options.to_nightly {
            VersionQuery::Nightly
        } else if let Some(ver) = &options.to_version {
            VersionQuery::Stable(Some(ver.clone()))
        } else {
            VersionQuery::Stable(None)
        };
        ToDo::InstanceUpgrade(name.into(), nver)
    } else if options.nightly {
        ToDo::NightlyUpgrade
    } else {
        ToDo::MinorUpgrade
    }
}

fn read_metadata(path: &Path) -> anyhow::Result<Metadata> {
    let file = fs::read(path)
        .with_context(|| format!("error reading {}", path.display()))?;
    let metadata = serde_json::from_slice(&file)
        .with_context(|| format!("error decoding json {}", path.display()))?;
    Ok(metadata)
}

fn get_instances<'x>(todo: &ToDo, methods: &'x Methods)
    -> anyhow::Result<Vec<(&'x dyn Method, Vec<Instance<'x>>)>>
{
    use ToDo::*;
    let mut result = Vec::new();
    for meth in methods.values() {
        let mut chunk = Vec::new();
        for instance in meth.all_instances()?.into_iter() {
            let include = match todo {
                MinorUpgrade => !instance.get_version()?.is_nightly(),
                NightlyUpgrade => instance.get_version()?.is_nightly(),
                InstanceUpgrade(name, ..) => instance.name() == name,
            };
            if include {
                chunk.push(Instance {
                    instance,
                    source: None,
                    version: None,
                });
            }
        }
        if !chunk.is_empty() {
            result.push((&**meth, chunk));
        }
    }
    Ok(result)
}

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    let todo = interpret_options(&options);
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;

    for meth in methods.values() {
        meth.upgrade(&todo, options)?;
    }
    Ok(())
}

/*

    let instances = get_instances(&todo, &methods)?;
    if instances.is_empty() {
        if options.nightly {
            log::warn!(target: "edgedb::server::upgrade",
                "No instances found. Nothing to upgrade.");
        } else {
            log::warn!(target: "edgedb::server::upgrade",
                "No instances found. Nothing to upgrade \
                (Note: nightly instances are upgraded only if `--nightly` \
                is specified).");
        }
        return Ok(());
    }

    for (meth, instances) in instances {
        match todo {
            MinorUpgrade => {
                do_minor_upgrade(meth, instances, options)?;
            }
            NightlyUpgrade => {
                do_nightly_upgrade(meth, instances, options)?;
            }
            InstanceUpgrade(.., ref version) => {
                for inst in instances {
                    do_instance_upgrade(meth, inst, version, options)?;
                }
            }
        }
    }
    Ok(())
}
*/

async fn dump_instance(inst: &Instance<'_>, socket: &Path)
    -> anyhow::Result<()>
{
    todo!();
    /*
    log::info!(target: "edgedb::server::upgrade",
        "Dumping instance {:?}", inst.name());
    let path = data_path(false)?.join(format!("{}.dump", inst.name()));
    if path.exists() {
        log::info!(target: "edgedb::server::upgrade",
            "Removing old dump at {}", path.display());
        fs::remove_dir_all(&path)?;
    }
    let mut conn_params = client::Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(socket);
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::dump_all(&mut cli, &options, path.as_ref()).await?;
    Ok(())
    */
}

async fn restore_instance(inst: &Instance<'_>, socket: &Path)
    -> anyhow::Result<()>
{
    todo!();
    /*
    use crate::commands::parser::Restore;

    log::info!(target: "edgedb::server::upgrade",
        "Restoring instance {:?}", inst.name());
    let path = inst.data_dir.with_file_name(format!("{}.dump", inst.name()));
    let mut conn_params = client::Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(socket);
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::restore_all(&mut cli, &options, &Restore {
        path,
        all: true,
        allow_non_empty: false,
        verbose: false,
    }).await?;
    Ok(())
    */
}

fn do_nightly_upgrade(method: &dyn Method,
    mut instances: Vec<Instance>, options: &Upgrade)
    -> anyhow::Result<()>
{
    let instances_str = instances
        .iter().map(|inst| inst.name()).collect::<Vec<_>>().join(", ");

    let version_query = VersionQuery::Nightly;
    let new = method.get_version(&version_query)
        .context("Unable to determine version")?;
    let old = get_installed(&version_query, method)?;

    if !options.force {
        if let Some(old_ver) = &old {
            if old_ver >= &new.version() {
                log::info!(target: "edgedb::server::upgrade",
                    "Nightly is up to date {}, skipping instances: {}",
                    old_ver, instances_str);
                return Ok(());
            }
        }
    }
    for inst in &mut instances {
        inst.source = old.clone();
        inst.version = Some(new.version().clone());
    }

    for inst in &instances {
        dump_and_stop(inst)?;
    }

    let new_major = new.major_version().clone();
    log::info!(target: "edgedb::server::upgrade", "Upgrading the package");
    method.install(&install::Settings {
        method: method.name(),
        distribution: new,
        extra: LinkedHashMap::new(),
    })?;

    for inst in instances {
        reinit_and_restore(&inst, &new_major, method)?;
    }
    Ok(())
}

#[context("failed to dump {:?}", inst.name())]
fn dump_and_stop(inst: &Instance) -> anyhow::Result<()> {
    // in case not started for now
    log::info!(target: "edgedb::server::upgrade",
        "Ensuring instance is started");
    inst.instance.start(&options::Start {
        name: inst.name().into(),
        foreground: false,
    })?;
    task::block_on(dump_instance(inst, &inst.instance.get_socket(true)?))?;
    log::info!(target: "edgedb::server::upgrade",
        "Stopping the instance before package upgrade");
    inst.instance.stop(&options::Stop { name: inst.name().into() })?;
    Ok(())
}

#[context("failed to restore {:?}", inst.name())]
fn reinit_and_restore(inst: &Instance, version: &MajorVersion,
    method: &dyn Method)
    -> anyhow::Result<()>
{
    todo!();
    /*
    let base = inst.data_dir.parent().unwrap();
    let backup = base.join(&format!("{}.backup", inst.name()));
    fs::rename(&inst.data_dir, &backup)?;
    write_backup_meta(&backup.join("backup.json"), &BackupMeta {
        timestamp: SystemTime::now(),
    })?;

    let meta = inst.upgrade_meta();
    init(&options::Init {
        name: inst.name().into(),
        system: false,
        interactive: false,
        nightly: version.is_nightly(),
        version: version.as_stable().cloned(),
        method: Some(method.name()),
        port: Some(inst.instance.get_port()?),
        start_conf: inst.instance.get_start_conf()?,
        inhibit_user_creation: true,
        inhibit_start: true,
        upgrade_marker: Some(serde_json::to_string(&meta).unwrap()),
        overwrite: true,
        default_user: "edgedb".into(),
        default_database: "edgedb".into(),
    })?;

    let mut ctl = inst.get_control()?;
    let mut cmd = ctl.run_command()?;
    // temporarily patch the edgedb issue of 1-alpha.4
    cmd.arg("--default-database=edgedb");
    cmd.arg("--default-database-user=edgedb");
    log::debug!("Running server: {:?}", cmd);
    let child = ProcessGuard::run(&mut cmd)
        .with_context(|| format!("error running server {:?}", cmd))?;

    task::block_on(restore_instance(inst, &ctl.get_socket(true)?))?;
    log::info!(target: "edgedb::server::upgrade",
        "Restarting instance {:?} to apply changes from `restore --all`",
        &inst.name());
    drop(child);

    ctl.start(&options::Start { name: inst.name().into(), foreground: false })?;
    Ok(())
    */
}

pub fn get_installed(version: &VersionQuery, method: &dyn Method)
    -> anyhow::Result<Option<Version<String>>>
{
    for ver in method.installed_versions()? {
        if !version.distribution_matches(&ver) {
            continue
        }
        return Ok(Some(ver.version().clone()));
    }
    return Ok(None);
}

fn do_instance_upgrade(method: &dyn Method,
    mut inst: Instance, version: &VersionQuery, options: &Upgrade)
    -> anyhow::Result<()>
{
    let new = method.get_version(&version)
        .context("Unable to determine version")?;
    let old = get_installed(version, method)?;

    if !options.force {
        if let Some(old_ver) = &old {
            if old_ver >= &new.version() {
                log::info!(target: "edgedb::server::upgrade",
                    "Version {} is up to date {}, skipping instance: {}",
                    version, old_ver, inst.name());
                return Ok(());
            }
        }
    }
    inst.source = old;
    inst.version = Some(new.version().clone());

    dump_and_stop(&inst)?;

    let new_major = new.major_version().clone();
    log::info!(target: "edgedb::server::upgrade", "Installing the package");
    method.install(&install::Settings {
        method: method.name(),
        distribution: new,
        extra: LinkedHashMap::new(),
    })?;

    reinit_and_restore(&inst, &new_major, method)?;
    Ok(())
}

#[context("failed to write backup metadata file {}", path.display())]
fn write_backup_meta(path: &Path, metadata: &BackupMeta)
    -> anyhow::Result<()>
{
    fs::write(path, serde_json::to_vec(&metadata)?)?;
    Ok(())
}

impl Instance<'_> {
    fn name(&self) -> &str {
        self.instance.name()
    }
    fn upgrade_meta(&self) -> UpgradeMeta {
        UpgradeMeta {
            source: self.source.clone().unwrap_or(Version("unknown".into())),
            target: self.version.clone().unwrap_or(Version("unknown".into())),
            started: SystemTime::now(),
            pid: process::id(),
        }
    }
}
