use std::fs;
use std::path::Path;
use std::time::{SystemTime, Duration};

use async_std::task;
use fn_error_context::context;
use serde::{Serialize, Deserialize};

use edgedb_client as client;
use crate::server::detect::{self, VersionQuery};
use crate::server::options::{Upgrade, Start, Stop};
use crate::server::os_trait::{Method, Instance};
use crate::server::version::Version;
use crate::server::upgrade;
use crate::commands;


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

pub enum ToDo {
    MinorUpgrade,
    InstanceUpgrade(String, VersionQuery),
    NightlyUpgrade,
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

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    let todo = interpret_options(&options);
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;

    for meth in methods.values() {
        meth.upgrade(&todo, options)?;
    }
    Ok(())
}

pub async fn dump_instance(inst: &dyn Instance, destination: &Path,
    mut conn_params: client::Builder)
    -> anyhow::Result<()>
{
    log::info!(target: "edgedb::server::upgrade",
        "Dumping instance {:?}", inst.name());
    if destination.exists() {
        log::info!(target: "edgedb::server::upgrade",
            "Removing old dump at {}", destination.display());
        fs::remove_dir_all(&destination)?;
    }
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::dump_all(&mut cli, &options, destination.as_ref()).await?;
    Ok(())
}

pub async fn restore_instance(inst: &dyn Instance,
    path: &Path, mut conn_params: client::Builder)
    -> anyhow::Result<()>
{
    use crate::commands::parser::Restore;

    log::info!(target: "edgedb::server::upgrade",
        "Restoring instance {:?}", inst.name());
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::restore_all(&mut cli, &options, &Restore {
        path: path.into(),
        all: true,
        allow_non_empty: false,
        verbose: false,
    }).await?;
    Ok(())
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

#[context("failed to write backup metadata file {}", path.display())]
pub fn write_backup_meta(path: &Path, metadata: &BackupMeta)
    -> anyhow::Result<()>
{
    fs::write(path, serde_json::to_vec(&metadata)?)?;
    Ok(())
}

#[context("failed to dump {:?} -> {}", inst.name(), path.display())]
pub fn dump_and_stop(inst: &dyn Instance, path: &Path) -> anyhow::Result<()> {
    // in case not started for now
    log::info!(target: "edgedb::server::upgrade",
        "Ensuring instance is started");
    inst.start(&Start { name: inst.name().into(), foreground: false })?;
    task::block_on(
        upgrade::dump_instance(inst, &path, inst.get_connector(false)?))?;
    log::info!(target: "edgedb::server::upgrade",
        "Stopping the instance before package upgrade");
    inst.stop(&Stop { name: inst.name().into() })?;
    Ok(())
}
