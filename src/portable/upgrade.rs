use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, Duration};

use anyhow::Context;
use fn_error_context::context;

use crate::commands::{self, ExitCode};
use crate::cloud;
use crate::connect::{Connector, Connection};
use crate::portable::control;
use crate::portable::create;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{InstanceInfo, InstallInfo, Paths, write_json};
use crate::portable::options::{Upgrade, instance_arg, InstanceName};
use crate::portable::project;
use crate::portable::repository::{self, Query, PackageInfo, Channel};
use crate::portable::ver;
use crate::portable::windows;
use crate::print::{self, echo, Highlight};


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

pub fn print_project_upgrade_command(version: &Query,
    current_project: &Option<PathBuf>, project_dir: &Path)
{
    eprintln!(
        "  edgedb project upgrade {}{}",
        match version.channel {
            Channel::Stable => if let Some(filt) = &version.version {
                format!("--to-version={}", filt)
            } else {
                "--to-latest".into()
            },
            Channel::Nightly => "--to-nightly".into(),
            Channel::Testing => "--to-testing".into(),
        },
        if current_project.as_ref().map_or(false, |p| p == project_dir) {
            "".into()
        } else {
            format!(" --project-dir '{}'", project_dir.display())
        }
    );
}

fn check_project(name: &str, force: bool, ver_query: &Query)
    -> anyhow::Result<()>
{
    let project_dirs = project::find_project_dirs_by_instance(&name)?;
    if project_dirs.is_empty() {
        return Ok(())
    }

    project::print_instance_in_use_warning(&name, &project_dirs);
    let current_project = project::project_dir_opt(None)?;

    if force {
        eprintln!(
            "To update the project{} after the instance upgrade, run:",
            if project_dirs.len() > 1 { "s" } else { "" }
        );
    } else {
        eprintln!("To continue with the upgrade, run:");
    }
    for pd in project_dirs {
        let pd = project::read_project_path(&pd)?;
        print_project_upgrade_command(&ver_query, &current_project, &pd);
    }
    if !force {
        anyhow::bail!("Upgrade aborted.");
    }
    Ok(())
}

pub fn upgrade(cmd: &Upgrade, opts: &crate::options::Options) -> anyhow::Result<()> {
    match instance_arg(&cmd.name, &cmd.instance)? {
        InstanceName::Local(name) => upgrade_local(cmd, name),
        InstanceName::Cloud { org_slug: org, name } => upgrade_cloud(cmd, org, name, opts),
    }
}

fn upgrade_local(cmd: &Upgrade, name: &str) -> anyhow::Result<()> {
    let inst = InstanceInfo::read(name)?;
    let inst_ver = inst.get_version()?.specific();
    let (ver_query, ver_option) = Query::from_options(
        repository::QueryOptions {
            stable: cmd.to_latest,
            nightly: cmd.to_nightly,
            testing: cmd.to_testing,
            channel: cmd.to_channel,
            version: cmd.to_version.as_ref(),
        },
        || Query::from_version(&inst_ver),
    )?;
    check_project(name, cmd.force, &ver_query)?;

    if cfg!(windows) {
        return windows::upgrade(cmd, name);
    }

    let pkg = repository::get_server_package(&ver_query)?
        .context("no package found according to your criteria")?;
    let pkg_ver = pkg.version.specific();

    if pkg_ver <= inst_ver && !cmd.force {
        echo!("Latest version found", pkg.version.to_string() + ",",
              "current instance version is",
              inst.get_version()?.emphasize().to_string() + ".",
              "Already up to date.");
        return Ok(());
    }

    let inst = InstanceInfo::read(name)?;
    // When force is used we might upgrade to the same version, so
    // we rely on presence of the version specifying options instead to
    // define how we want upgrade to be performed. This is mostly useful
    // for tests.
    if pkg_ver.is_compatible(&inst_ver) && !(cmd.force && ver_option) &&
        !cmd.force_dump_restore
    {
        upgrade_compatible(inst, pkg)
    } else {
        upgrade_incompatible(inst, pkg)
    }
}

fn upgrade_cloud(cmd: &Upgrade, org: &str, name: &str, opts: &crate::options::Options) -> anyhow::Result<()> {
    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let inst = cloud::ops::find_cloud_instance_by_name(name, org, &client)?
        .ok_or_else(|| anyhow::anyhow!("instance not found"))?;

    let inst_ver = ver::Specific::from_str(&inst.version)?;
    let (ver_query, _) = Query::from_options(
        repository::QueryOptions {
            stable: cmd.to_latest,
            nightly: cmd.to_nightly,
            testing: cmd.to_testing,
            channel: cmd.to_channel,
            version: cmd.to_version.as_ref(),
        },
        || Query::from_version(&inst_ver),
    )?;

    let pkg = repository::get_server_package(&ver_query)?
        .context("no package found according to your criteria")?;
    let pkg_ver = pkg.version.specific();

    if pkg_ver <= inst_ver && !cmd.force {
        echo!("Latest version found", pkg.version.to_string() + ",",
            "current instance version is",
            inst.version.emphasize().to_string() + ".",
            "Already up to date.");
        return Ok(());
    }

    let request = cloud::ops::CloudInstanceUpgrade {
        org: org.to_string(),
        name: name.to_string(),
        version: pkg_ver.to_string(),
    };
    cloud::ops::upgrade_cloud_instance(&client, &request)?;

    print::echo!(
        "EdgeDB Cloud instance",
        format!("{}/{}", org, name).emphasize(),
        "is successfully upgraded.",
    );
    Ok(())
}

pub fn upgrade_compatible(mut inst: InstanceInfo, pkg: PackageInfo)
    -> anyhow::Result<()>
{
    echo!("Upgrading to a minor version", pkg.version.emphasize());
    let install = install::package(&pkg).context("error installing EdgeDB")?;
    inst.installation = Some(install);

    let metapath = inst.data_dir()?.join("instance_info.json");
    write_json(&metapath, "new instance metadata", &inst)?;

    create::create_service(&inst)
        .map_err(|e| {
            log::warn!("Error running EdgeDB as a service: {e:#}");
        }).ok();
    control::do_restart(&inst)?;
    echo!("Instance", inst.name.emphasize(),
          "is successfully upgraded to", pkg.version.emphasize());
    Ok(())
}

pub fn upgrade_incompatible(mut inst: InstanceInfo, pkg: PackageInfo)
    -> anyhow::Result<()>
{
    echo!("Upgrading to a major version", pkg.version.emphasize());
    let install = install::package(&pkg).context("error installing EdgeDB")?;

    let paths = Paths::get(&inst.name)?;
    dump_and_stop(&inst, &paths.dump_path)?;

    backup(&inst, &install, &paths)?;

    inst.installation = Some(install);

    reinit_and_restore(&inst, &paths).map_err(|e| {
        print::error(format!("{:#}", e));
        eprintln!("To undo run:\n  edgedb instance revert -I {:?}", inst.name);
        ExitCode::new(exit_codes::NEEDS_REVERT)
    })?;

    fs::remove_file(&paths.upgrade_marker)
        .with_context(|| format!("removing {:?}", paths.upgrade_marker))?;

    create::create_service(&inst)
        .map_err(|e| {
            log::warn!("Error running EdgeDB as a service: {e:#}");
        }).ok();
    control::do_restart(&inst)?;
    echo!("Instance", inst.name.emphasize(),
           "is successfully upgraded to", pkg.version.emphasize());

    Ok(())
}

#[context("cannot dump {:?} -> {}", inst.name, path.display())]
pub fn dump_and_stop(inst: &InstanceInfo, path: &Path) -> anyhow::Result<()> {
    // in case not started for now
    echo!("Dumping the database...");
    log::info!("Ensuring instance is started");
    let res = control::do_start(&inst);
    if let Err(err) = res {
        log::warn!("Error starting service: {:#}. Trying to start manually.",
            err);
        control::ensure_runstate_dir(&inst.name)?;
        let mut cmd = control::get_server_cmd(inst, false)?;
        cmd.background_for(|| Ok(dump_instance(inst, &path)))?;
    } else {
        block_on_dump_instance(inst, &path)?;
        log::info!("Stopping the instance before executable upgrade");
        control::do_stop(&inst.name)?;
    }
    Ok(())
}

#[tokio::main]
async fn block_on_dump_instance(inst: &InstanceInfo, destination: &Path)
    -> anyhow::Result<()>
{
    dump_instance(inst, destination).await
}

#[context("error dumping instance")]
pub async fn dump_instance(inst: &InstanceInfo, destination: &Path)
    -> anyhow::Result<()>
{
    use tokio::fs;

    let destination = Path::new(destination);
    log::info!("Dumping instance {:?}", inst.name);
    if fs::metadata(&destination).await.is_ok() {
        log::info!("Removing old dump at {}", destination.display());
        fs::remove_dir_all(&destination).await?;
    }
    let conn_params = inst.admin_conn_params().await?;
    let mut cli = Connection::connect(&conn_params.build()?).await?;
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
        source: inst.get_version()?.clone(),
        target: new_inst.version.clone(),
        started: SystemTime::now(),
        pid: std::process::id(),
    })?;

    write_json(&paths.data_dir.join("backup.json"), "backup metadata",
        &BackupMeta { timestamp: SystemTime::now() })?;
    if paths.backup_dir.exists() {
        fs_err::remove_dir_all(&paths.backup_dir)?;
    }
    fs_err::rename(&paths.data_dir, &paths.backup_dir)?;

    Ok(())
}

#[context("cannot restore {:?}", inst.name)]
fn reinit_and_restore(inst: &InstanceInfo, paths: &Paths) -> anyhow::Result<()>
{
    fs::create_dir_all(&paths.data_dir)
        .with_context(|| format!("cannot create {:?}", paths.data_dir))?;

    echo!("Restoring the database...");
    control::ensure_runstate_dir(&inst.name)?;
    let mut cmd = control::get_server_cmd(inst, false)?;
    control::self_signed_arg(&mut cmd, inst.get_version()?);
    cmd.background_for(|| Ok(async {
        restore_instance(inst, &paths.dump_path).await?;
        log::info!("Restarting instance {:?} to apply \
                   changes from `restore --all`",
                   &inst.name);
        Ok(())
    }))?;

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
    let mut conn_params = inst.admin_conn_params().await?;
    conn_params.wait_until_available(Duration::from_secs(300));

    log::info!("Restoring instance {:?}", inst.name);
    let mut cli = Connection::connect(&conn_params.build()?).await?;

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
