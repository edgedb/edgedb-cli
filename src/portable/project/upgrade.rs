use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Context;
use clap::ValueHint;
use gel_tokio::get_stash_path;

use crate::branding::{BRANDING, BRANDING_CLI_CMD, CONFIG_FILE_DISPLAY_NAME};
use crate::cloud;
use crate::cloud::client::CloudClient;
use crate::migrations;
use crate::portable::config;
use crate::portable::instance;
use crate::portable::instance::upgrade;
use crate::portable::local::InstanceInfo;
use crate::portable::options::InstanceName;
use crate::portable::project;
use crate::portable::repository::{self, Channel, Query};
use crate::portable::ver;
use crate::portable::windows;
use crate::print::{self, msg, Highlight};
use crate::question;

pub fn run(options: &Command, opts: &crate::options::Options) -> anyhow::Result<()> {
    let (query, version_set) = Query::from_options(
        repository::QueryOptions {
            nightly: options.to_nightly,
            stable: options.to_latest,
            testing: options.to_testing,
            version: options.to_version.as_ref(),
            channel: options.to_channel,
        },
        || Ok(Query::stable()),
    )?;
    if version_set {
        update_toml(options, opts, query)
    } else {
        upgrade_instance(options, opts)
    }
}

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    /// Explicitly set a root directory for the project
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Upgrade specified instance to latest version
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_testing", "to_nightly", "to_channel",
    ])]
    pub to_latest: bool,

    /// Upgrade specified instance to a specified version.
    ///
    /// e.g. --to-version 4.0-beta.1
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_testing", "to_latest", "to_nightly", "to_channel",
    ])]
    pub to_version: Option<ver::Filter>,

    /// Upgrade specified instance to latest nightly version
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_latest", "to_testing", "to_channel",
    ])]
    pub to_nightly: bool,

    /// Upgrade specified instance to latest testing version
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_latest", "to_nightly", "to_channel",
    ])]
    pub to_testing: bool,

    /// Upgrade specified instance to the specified channel
    #[arg(long, value_enum)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_latest", "to_nightly", "to_testing",
    ])]
    pub to_channel: Option<Channel>,

    /// Verbose output
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Force upgrade process even if there is no new version
    #[arg(long)]
    pub force: bool,

    /// Do not ask questions, assume user wants to upgrade instance
    #[arg(long)]
    pub non_interactive: bool,
}

pub fn update_toml(
    options: &Command,
    opts: &crate::options::Options,
    query: Query,
) -> anyhow::Result<()> {
    let Some((root, config_path)) = project::project_dir(options.project_dir.as_deref())? else {
        anyhow::bail!("`{CONFIG_FILE_DISPLAY_NAME}` not found, unable to upgrade {BRANDING} instance without an initialized project.");
    };
    let config = config::read(&config_path)?;
    let schema_dir = &config.project.schema_dir;

    let pkg = repository::get_server_package(&query)?.with_context(|| {
        format!(
            "cannot find package matching {} \
            (Use `{BRANDING_CLI_CMD} server list-versions` to see all available)",
            query.display()
        )
    })?;
    let pkg_ver = pkg.version.specific();

    let stash_dir = get_stash_path(&root)?;
    if !stash_dir.exists() {
        log::warn!("No associated instance found.");

        if config::modify_server_ver(&config_path, &query)? {
            print::success!("Config updated successfully.");
        } else {
            print::success!("Config is up to date.");
        }
        msg!(
            "Run {} {} to initialize an instance.",
            BRANDING_CLI_CMD,
            " project init".command_hint()
        );
    } else {
        let name = project::instance_name(&stash_dir)?;
        let database = project::database_name(&stash_dir)?;
        let client = CloudClient::new(&opts.cloud_options)?;
        let mut inst = project::Handle::probe(&name, &root, schema_dir, &client)?;
        inst.database = database;

        let result = match inst.instance {
            project::InstanceKind::Remote => anyhow::bail!("remote instances cannot be upgraded"),
            project::InstanceKind::Portable(inst) => {
                upgrade_local(options, &config, inst, &query, opts)
            }
            project::InstanceKind::Wsl => todo!(),
            project::InstanceKind::Cloud { org_slug, name, .. } => {
                upgrade_cloud(options, &org_slug, &name, &query, opts)
            }
        }?;

        match result.action {
            upgrade::UpgradeAction::Upgraded => {
                let config_version = if query.is_nightly() {
                    query.clone()
                } else {
                    // on `--to-latest` which is equivalent to `server-version="*"`
                    // we put specific version instead
                    Query::from_version(&pkg_ver)?
                };

                if config::modify_server_ver(&config_path, &config_version)? {
                    msg!("Remember to commit it to version control.");
                }
                let name_str = name.to_string();
                print_other_project_warning(&name_str, &root, &query)?;
            }
            upgrade::UpgradeAction::Cancelled => {
                msg!("Canceled.");
            }
            upgrade::UpgradeAction::None => {
                msg!("Already up to date.\nRequested upgrade version is {} current instance version is {}", result.requested_version.emphasize().to_string() + ",", result.prior_version.emphasize().to_string() + ".");
            }
        }
    };

    Ok(())
}

fn print_other_project_warning(
    name: &str,
    project_path: &Path,
    to_version: &Query,
) -> anyhow::Result<()> {
    let mut project_dirs = Vec::new();
    for pd in project::find_project_dirs_by_instance(name)? {
        let real_pd = match project::read_project_path(&pd) {
            Ok(path) => path,
            Err(e) => {
                print::error!("{e}");
                continue;
            }
        };
        if real_pd != project_path {
            project_dirs.push(real_pd);
        }
    }
    if !project_dirs.is_empty() {
        print::warn!(
            "Warning: the instance {name} is still used by the following \
            projects:"
        );
        for pd in &project_dirs {
            eprintln!("  {}", pd.display());
        }
        eprintln!("Run the following commands to update them:");
        for pd in &project_dirs {
            instance::upgrade::print_project_upgrade_command(to_version, &None, pd);
        }
    }
    Ok(())
}

pub fn upgrade_instance(options: &Command, opts: &crate::options::Options) -> anyhow::Result<()> {
    let Some((root, config_path)) = project::project_dir(options.project_dir.as_deref())? else {
        anyhow::bail!("`{CONFIG_FILE_DISPLAY_NAME}` not found, unable to upgrade {BRANDING} instance without an initialized project.");
    };
    let config = config::read(&config_path)?;
    let cfg_ver = &config.instance.server_version;
    let schema_dir = &config.project.schema_dir;

    let stash_dir = get_stash_path(&root)?;
    if !stash_dir.exists() {
        anyhow::bail!("No instance initialized.");
    }

    let instance_name = project::instance_name(&stash_dir)?;
    let database = project::database_name(&stash_dir)?;
    let client = CloudClient::new(&opts.cloud_options)?;
    let mut inst = project::Handle::probe(&instance_name, &root, schema_dir, &client)?;
    inst.database = database;
    let result = match inst.instance {
        project::InstanceKind::Remote => anyhow::bail!("remote instances cannot be upgraded"),
        project::InstanceKind::Portable(inst) => {
            upgrade_local(options, &config, inst, cfg_ver, opts)
        }
        project::InstanceKind::Wsl => todo!(),
        project::InstanceKind::Cloud { org_slug, name, .. } => {
            upgrade_cloud(options, &org_slug, &name, cfg_ver, opts)
        }
    }?;

    match result.action {
        upgrade::UpgradeAction::Upgraded => {
            // When upgrade attempt was made, implementations
            // would have already printed a message.
        }
        upgrade::UpgradeAction::Cancelled => {
            msg!("Canceled.");
        }
        upgrade::UpgradeAction::None => {
            msg!(
                "{BRANDING} instance is up to date with \
                the specification in `{}`.",
                config_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            if let Some(available) = result.available_upgrade {
                msg!("New major version is available: {}", available.emphasize());
                msg!(
                    "To update `{}` and upgrade to this version, \
                        run:\n    {} project upgrade --to-latest",
                    BRANDING_CLI_CMD,
                    config_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                );
            }
        }
    }

    Ok(())
}

fn upgrade_local(
    cmd: &Command,
    config: &config::Config,
    inst: InstanceInfo,
    to_version: &Query,
    opts: &crate::options::Options,
) -> anyhow::Result<upgrade::UpgradeResult> {
    let inst_ver = inst.get_version()?.specific();

    let instance_name = InstanceName::from_str(&inst.name)?;
    let pkg = repository::get_server_package(to_version)?.with_context(|| {
        format!(
            "cannot find package matching {} \
            (Use `{BRANDING_CLI_CMD} server list-versions` to see all available)",
            to_version.display()
        )
    })?;
    let pkg_ver = pkg.version.specific();

    if pkg_ver > inst_ver || cmd.force {
        if cfg!(windows) {
            windows::upgrade(
                &instance::upgrade::Command {
                    to_latest: false,
                    to_version: to_version.version.clone(),
                    to_channel: None,
                    to_nightly: false,
                    to_testing: false,
                    name: None,
                    instance: Some(instance_name),
                    verbose: false,
                    force: cmd.force,
                    force_dump_restore: cmd.force,
                    non_interactive: true,
                    cloud_opts: opts.cloud_options.clone(),
                },
                &inst.name,
            )?;
        } else {
            ver::print_version_hint(&pkg_ver, to_version);
            // When force is used we might upgrade to the same version, but
            // since some selector like `--to-latest` was specified we assume
            // user want to treat this upgrade as incompatible and do the
            // upgrade. This is mostly for testing.
            if pkg_ver.is_compatible(&inst_ver) && !cmd.force {
                upgrade::upgrade_compatible(inst, pkg)?;
            } else {
                migrations::upgrade_check::to_version(&pkg, config)?;
                upgrade::upgrade_incompatible(inst, pkg, cmd.non_interactive)?;
            }
        }
        Ok(upgrade::UpgradeResult {
            action: upgrade::UpgradeAction::Upgraded,
            prior_version: inst_ver,
            requested_version: pkg_ver,
            available_upgrade: None,
        })
    } else {
        let mut available_upgrade = None;
        if to_version.channel != Channel::Nightly {
            if let Some(pkg) = repository::get_server_package(&Query::stable())? {
                let sv = pkg.version.specific();
                if sv > inst_ver {
                    available_upgrade = Some(sv);
                }
            }
        }

        Ok(upgrade::UpgradeResult {
            action: upgrade::UpgradeAction::None,
            prior_version: inst_ver,
            requested_version: pkg_ver,
            available_upgrade,
        })
    }
}

fn upgrade_cloud(
    cmd: &Command,
    org: &str,
    name: &str,
    to_version: &Query,
    opts: &crate::options::Options,
) -> anyhow::Result<upgrade::UpgradeResult> {
    let client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let result = upgrade::upgrade_cloud(org, name, to_version, &client, cmd.force, |target_ver| {
        let target_ver_str = target_ver.to_string();
        let _inst_name = format!("{org}/{name}");
        let inst_name = _inst_name.emphasize();
        if !cmd.non_interactive {
            question::Confirm::new(format!(
                "This will upgrade {inst_name} to version {target_ver_str}.\
                    \nConfirm upgrade?",
            ))
            .ask()
        } else {
            Ok(true)
        }
    })?;

    if let upgrade::UpgradeAction::Upgraded = result.action {
        let inst_name = format!("{org}/{name}");
        msg!(
            "Instance {} has been successfully upgraded to {}",
            inst_name.emphasize(),
            result.requested_version.emphasize().to_string() + "."
        );
    }

    Ok(result)
}
