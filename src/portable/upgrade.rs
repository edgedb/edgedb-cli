use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::commands::ExitCode;
use crate::portable::control;
use crate::portable::create;
use crate::portable::install;
use crate::portable::local::InstanceInfo;
use crate::portable::project;
use crate::portable::repository::{self, Query, PackageInfo};

use crate::print::{eecho, Highlight};
use crate::server::options::{Upgrade, StartConf};


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
    eecho!("Upgraing to minor version", pkg.version.emphasize());
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

fn upgrade_incompatible(_inst: InstanceInfo, _pkg: PackageInfo)
    -> anyhow::Result<()>
{
    todo!();
}
