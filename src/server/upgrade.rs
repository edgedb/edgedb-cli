use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration};

use async_std::task;
use fn_error_context::context;
use serde::{Serialize, Deserialize};

use edgedb_client as client;
use crate::commands;
use crate::connect::Connector;
use crate::hint::HintExt;
use crate::print;
use crate::project;
use crate::server::destroy;
use crate::server::detect;
use crate::server::errors::InstanceNotFound;
use crate::server::options::{Upgrade, Start, Stop};
use crate::server::os_trait::{Method, Instance};
use crate::server::version::{Version, VersionQuery};


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
    InstanceUpgrade(String, Option<VersionQuery>),
}

fn interpret_options(options: &Upgrade) -> anyhow::Result<ToDo> {
    if options.local_minor {
        if options.name.is_some() {
            Err(anyhow::anyhow!(
                "Cannot perform minor version upgrade on a single instance"
            )).hint(
                "Run `edgedb instance upgrade --local-minor` without \
                specifying an instance.")?;
        }
        Ok(ToDo::MinorUpgrade)
    } else if let Some(name) = &options.name {
        let nver = if options.to_nightly {
            if options.to_latest {
                anyhow::bail!(
                    "--to-nightly and --to-latest are mutually exclusive"
                )
            }
            if options.to_version.is_some() {
                anyhow::bail!(
                    "--to-nightly and --to-version are mutually exclusive"
                )
            }
            Some(VersionQuery::Nightly)
        } else if options.to_latest {
            if options.to_version.is_some() {
                anyhow::bail!(
                    "--to-latest and --to-version are mutually exclusive"
                )
            }
            None
        } else if let Some(ver) = &options.to_version {
            Some(VersionQuery::Stable(Some(ver.clone())))
        } else {
            Err(anyhow::anyhow!("No upgrade operation specified."))
                .hint("Use one of `--to-latest`, `--to-version` or \
                      `--to-nightly`.")?
        };
        Ok(ToDo::InstanceUpgrade(name.into(), nver))
    } else {
        Err(anyhow::anyhow!("No upgrade operation specified."))
            .hint("Use `--local-minor` to upgrade the minor version of all \
                   local instances, or specify name of the instance \
                   to upgrade.")?
    }
}

pub fn print_project_upgrade_command(
    version: &str, current_project: &Option<PathBuf>, project_dir: &Path
) {
    eprintln!(
        "  edgedb project upgrade {}{}",
        version,
        if current_project.as_ref().map_or(false, |p| p == project_dir) {
            "".into()
        } else {
            format!(" --project-dir '{}'", project_dir.display())
        }
    );
}

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    let todo = interpret_options(&options)?;
    if let ToDo::InstanceUpgrade(name, version) = &todo {
        let project_dirs = destroy::find_project_dirs(name)?;
        if !project_dirs.is_empty() {
            destroy::print_instance_in_use_warning(name, &project_dirs);
            let current_project = project::project_dir_opt(None)?;
            let version = match version {
                | Some(VersionQuery::Stable(None))
                | None => "--to-latest".into(),
                Some(VersionQuery::Nightly) => "--to-nightly".into(),
                Some(VersionQuery::Stable(Some(version))) => {
                    format!("--to-version {}", version)
                }
            };
            if options.force {
                eprintln!(
                    "To update the project{} after the instance upgrade, run:",
                    if project_dirs.len() > 1 { "s" } else { "" }
                );
            } else {
                eprintln!("To continue with the upgrade, run:");
            }
            for pd in project_dirs {
                let pd = destroy::read_project_real_path(&pd)?;
                print_project_upgrade_command(&version, &current_project, &pd);
            }
            if !options.force {
                anyhow::bail!("Upgrade aborted.");
            }
        }
    }
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let mut errors = Vec::new();
    let mut any_upgraded = false;
    for meth in methods.values() {
        match meth.upgrade(&todo, options) {
            Ok(upgraded) if upgraded => {
                any_upgraded = true;
                if let ToDo::InstanceUpgrade(name, _version) = &todo {
                    let new_inst = meth.get_instance(name)?;
                    let version = new_inst.get_current_version()?.unwrap();
                    print::success_msg(
                        format!("Successfully upgraded EdgeDB instance \
                                      '{}' to version", name),
                        version,
                    );
                    break
                }
            }
            Ok(_) => {}
            Err(e) if e.is::<InstanceNotFound>() => {
                errors.push((meth.name(), e));
            }
            Err(e) => Err(e)?,
        }
    }
    if errors.len() == methods.len() {
        print::error("No instances found:");
        for (meth, err) in errors {
            eprintln!("  * {}: {:#}", meth.title(), err);
        }
        Err(commands::ExitCode::new(1).into())
    } else {
        match (any_upgraded, &todo) {
            (false, _) => {
                print::success("Already up to date.")
            }
            (true, ToDo::MinorUpgrade) => {
                print::success("Successfully upgraded minor versions of \
                               local EdgeDB instances.");
            }
            _ => {}
        }
        Ok(())
    }
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
        conn_params: Connector::new(Ok(conn_params)),
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
        conn_params: Connector::new(Ok(conn_params)),
    };
    commands::restore_all(&mut cli, &options, &Restore {
        path: path.into(),
        all: true,
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
    let res = inst.start(&Start {
        name: inst.name().into(),
        foreground: false,
    });
    if let Err(err) = res {
        log::warn!("Error starting service: {:#}. Trying to start manually.",
            err);
        let mut cmd = inst.get_command()?;
        cmd.background_for(
            dump_instance(inst, &path, inst.get_connector(false)?)
        )?;
    } else {
        task::block_on(
            dump_instance(inst, &path, inst.get_connector(false)?))?;
        log::info!(target: "edgedb::server::upgrade",
            "Stopping the instance before executable upgrade");
        inst.stop(&Stop { name: inst.name().into() })?;
    }
    Ok(())
}
