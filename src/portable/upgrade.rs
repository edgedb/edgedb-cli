use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;

use crate::commands::{self, ExitCode};
use crate::connect::Connector;
use crate::portable::control;
use crate::portable::create;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{InstanceInfo, InstallInfo, Paths, write_json};
use crate::portable::project;
use crate::portable::repository::{self, Query, PackageInfo};
use crate::portable::ver;
use crate::print::{self, eecho, Highlight};
use crate::server::options::{Upgrade, StartConf};


#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct UpgradeMeta {
    pub source: ver::Build,
    pub target: ver::Build,
    #[serde(with="humantime_serde")]
    pub started: SystemTime,
    pub pid: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BackupMeta {
    #[serde(with="humantime_serde")]
    pub timestamp: SystemTime,
}

fn print_project_upgrade_command(
    options: &Upgrade, current_project: &Option<PathBuf>, project_dir: &Path
) {
    eprintln!(
        "  edgedb project upgrade {}{}",
        if options.to_latest {
            "--to-latest".into()
        } else if options.to_nightly {
            "--to-nightly".into()
        } else if let Some(ver) = &options.to_version {
            format!("--version={}", ver.num())
        } else {
            "".into()
        },
        if current_project.as_ref().map_or(false, |p| p == project_dir) {
            "".into()
        } else {
            format!(" --project-dir '{}'", project_dir.display())
        }
    );
}

fn check_project(options: &Upgrade) -> anyhow::Result<()> {
    let project_dirs = project::find_project_dirs(&options.name)?;
    if project_dirs.is_empty() {
        return Ok(())
    }

    project::print_instance_in_use_warning(&options.name, &project_dirs);
    let current_project = project::project_dir_opt(None)?;

    if options.force {
        eprintln!(
            "To update the project{} after the instance upgrade, run:",
            if project_dirs.len() > 1 { "s" } else { "" }
        );
    } else {
        eprintln!("To continue with the upgrade, run:");
    }
    for pd in project_dirs {
        let pd = project::read_project_real_path(&pd)?;
        print_project_upgrade_command(&options, &current_project, &pd);
    }
    if !options.force {
        anyhow::bail!("Upgrade aborted.");
    }
    Ok(())
}

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    check_project(options)?;
    let inst = InstanceInfo::read(&options.name)?;
    let inst_ver = inst.installation.version.specific();
    let ver_option = options.to_latest || options.to_nightly ||
        options.to_version.is_some();
    let ver_query = if ver_option {
        Query::from_options(options.to_nightly, &options.to_version)?
    } else {
        Query::from_version(&inst_ver)?
    };

    let pkg = repository::get_server_package(&ver_query)?
        .context("no package found according to your criteria")?;
    let pkg_ver = pkg.version.specific();

    if pkg_ver <= inst_ver && !options.force {
        eecho!("Latest version found", pkg.version,
               ", current instance version is", inst.installation.version,
               ". Already up to date.");
        return Ok(());
    }

    // When force is used we might upgrade to the same version, so
    // we rely on presence of the version specifying options instead to
    // define how we want upgrade to be performed. This is mostly useful
    // for tests.
    if pkg_ver.is_compatible(&inst_ver) && !(options.force && ver_option) {
        upgrade_compatible(inst, pkg)
    } else {
        upgrade_incompatible(inst, pkg)
    }
}

fn upgrade_compatible(mut inst: InstanceInfo, pkg: PackageInfo)
    -> anyhow::Result<()>
{
    eecho!("Upgrading to a minor version", pkg.version.emphasize());
    let install = install::package(&pkg).context("error installing EdgeDB")?;
    inst.installation = install;
    match (create::create_service(&inst), inst.start_conf) {
        (Ok(()), StartConf::Manual) => {
            eecho!("Instance", inst.name.emphasize(),
                   "is upgraded to", pkg.version.emphasize());
            eprintln!("Please restart the server or run: \n  \
                edgedb instance start [--foreground] {}",
                inst.name);
        }
        (Ok(()), StartConf::Auto) => {
            control::do_restart(&inst)?;
            eecho!("Instance", inst.name.emphasize(),
                   "is successfully upgraded to", pkg.version.emphasize());
        }
        (Err(e), _) => {
            eecho!("Upgrade to", pkg.version.emphasize(), "is complete, \
                but there was an error creating the service:",
                format_args!("{:#}", e));
            eprintln!(": \n  \
                edgedb instance start --foreground {}",
                inst.name);
            return Err(ExitCode::new(2))?;
        }
    }
    Ok(())
}

fn upgrade_incompatible(mut inst: InstanceInfo, pkg: PackageInfo)
    -> anyhow::Result<()>
{
    eecho!("Upgrading to a major version", pkg.version.emphasize());
    let install = install::package(&pkg).context("error installing EdgeDB")?;

    let paths = Paths::get(&inst.name)?;
    dump_and_stop(&inst, &paths.dump_path)?;

    backup(&inst, &install, &paths)?;

    inst.installation = install;

    reinit_and_restore(&inst, &paths).map_err(|e| {
        print::error(format!("{:#}", e));
        eprintln!("To undo run:\n  edgedb instance revert {:?}", inst.name);
        ExitCode::new(exit_codes::NEEDS_REVERT)
    })?;

    fs::remove_file(&paths.upgrade_marker)
        .with_context(|| format!("removing {:?}", paths.upgrade_marker))?;

    match (create::create_service(&inst), inst.start_conf) {
        (Ok(()), StartConf::Manual) => {
            eecho!("Instance", inst.name.emphasize(),
                   "is upgraded to", pkg.version.emphasize());
            eprintln!("Please restart the server or run: \n  \
                edgedb instance start [--foreground] {}",
                inst.name);
        }
        (Ok(()), StartConf::Auto) => {
            control::do_restart(&inst)?;
            eecho!("Instance", inst.name.emphasize(),
                   "is successfully upgraded to", pkg.version.emphasize());
        }
        (Err(e), _) => {
            eecho!("Upgrade to", pkg.version.emphasize(), "is complete, \
                but there was an error creating the service:",
                format_args!("{:#}", e));
            eprintln!(": \n  \
                edgedb instance start --foreground {}",
                inst.name);
            return Err(ExitCode::new(2))?;
        }
    }

    Ok(())
}

#[context("cannot dump {:?} -> {}", inst.name, path.display())]
pub fn dump_and_stop(inst: &InstanceInfo, path: &Path) -> anyhow::Result<()> {
    // in case not started for now
    eecho!("Dumping the database...");
    log::info!("Ensuring instance is started");
    let res = control::do_start(&inst);
    if let Err(err) = res {
        log::warn!("Error starting service: {:#}. Trying to start manually.",
            err);
        let mut cmd = control::get_server_cmd(inst)?;
        cmd.background_for(dump_instance(inst, &path))?;
    } else {
        task::block_on(dump_instance(inst, &path))?;
        log::info!("Stopping the instance before executable upgrade");
        control::do_stop(inst)?;
    }
    Ok(())
}

#[context("error dumping instance")]
pub async fn dump_instance(inst: &InstanceInfo, destination: &Path)
    -> anyhow::Result<()>
{
    use async_std::fs;
    use async_std::path::Path;

    let destination = Path::new(destination);
    log::info!("Dumping instance {:?}", inst.name);
    if destination.exists().await {
        log::info!("Removing old dump at {}", destination.display());
        fs::remove_dir_all(&destination).await?;
    }
    let conn_params = inst.admin_conn_params().await?;
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params: Connector::new(Ok(conn_params)),
    };
    commands::dump_all(&mut cli, &options, destination.as_ref()).await?;
    Ok(())
}

fn backup(inst: &InstanceInfo, new_inst: &InstallInfo, paths: &Paths)
    -> anyhow::Result<()>
{
    if paths.upgrade_marker.exists() {
        anyhow::bail!("Upgrade is already in progress");
    }
    write_json(&paths.upgrade_marker, "upgrade marker", &UpgradeMeta {
        source: inst.installation.version.clone(),
        target: new_inst.version.clone(),
        started: SystemTime::now(),
        pid: std::process::id(),
    })?;

    write_json(&paths.data_dir.join("backup.json"), "backup metadata",
        &BackupMeta { timestamp: SystemTime::now() })?;
    if paths.backup_dir.exists() {
        fs::remove_dir_all(&paths.backup_dir)?;
    }
    fs::rename(&paths.data_dir, &paths.backup_dir)?;

    Ok(())
}

#[context("cannot restore {:?}", inst.name)]
fn reinit_and_restore(inst: &InstanceInfo, paths: &Paths) -> anyhow::Result<()>
{
    fs::create_dir_all(&paths.data_dir)
        .with_context(|| format!("cannot create {:?}", paths.data_dir))?;

    eecho!("Restoring the database...");
    let mut cmd = control::get_server_cmd(inst)?;
    cmd.arg("--generate-self-signed-cert");
    cmd.background_for(async {
        restore_instance(inst, &paths.dump_path).await?;
        log::info!("Restarting instance {:?} to apply \
                   changes from `restore --all`",
                   &inst.name);
        Ok(())
    })?;

    let metapath = paths.data_dir.join("instance_info.json");
    write_json(&metapath, "new instance metadata", &inst)?;

    fs::copy(
        paths.backup_dir.join("edbtlscert.pem"),
        paths.data_dir.join("edbtlscert.pem")
    )?;
    fs::copy(
        paths.backup_dir.join("edbprivkey.pem"),
        paths.data_dir.join("edbprivkey.pem")
    )?;

    Ok(())
}

async fn restore_instance(inst: &InstanceInfo, path: &Path)
    -> anyhow::Result<()>
{
    use crate::commands::parser::Restore;
    let conn_params = inst.admin_conn_params().await?;

    log::info!("Restoring instance {:?}", inst.name);
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
