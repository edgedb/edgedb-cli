use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Context;
use clap::ValueHint;
use const_format::concatcp;
use edgedb_tokio::get_stash_path;
use edgedb_tokio::PROJECT_FILES;
use rand::{thread_rng, Rng};

use edgedb_errors::DuplicateDatabaseDefinitionError;
use edgeql_parser::helpers::quote_name;

use crate::branding::BRANDING_CLOUD;
use crate::branding::QUERY_TAG;
use crate::branding::{BRANDING, BRANDING_CLI_CMD, CONFIG_FILE_DISPLAY_NAME};
use crate::cloud;
use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::options::CloudOptions;
use crate::portable::config;
use crate::portable::exit_codes;
use crate::portable::instance::control;
use crate::portable::instance::create;
use crate::portable::local::{allocate_port, InstanceInfo, Paths};
use crate::portable::options::InstanceName;
use crate::portable::options::{CloudInstanceBillables, CloudInstanceParams};
use crate::portable::platform::optional_docker_check;
use crate::portable::project;
use crate::portable::repository::{self, Channel, PackageInfo, Query};
use crate::portable::server::install;
use crate::portable::ver;
use crate::portable::ver::Specific;
use crate::portable::windows;
use crate::print::{self, msg, Highlight};
use crate::question;
use crate::table;

#[allow(clippy::collapsible_else_if)]
pub fn run(options: &Command, opts: &crate::options::Options) -> anyhow::Result<()> {
    if optional_docker_check()? {
        print::error!("`{BRANDING_CLI_CMD} project init` is not supported in Docker containers.");
        Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }

    if options.server_start_conf.is_some() {
        print::warn!(
            "The option `--server-start-conf` is deprecated. \
                     Use `{BRANDING_CLI_CMD} instance start/stop` to control \
                     the instance."
        );
    }

    let project_dir = project::project_dir(options.project_dir.as_deref())?;

    if let Some((project_dir, config_path)) = project_dir {
        if options.link {
            link(options, &project_dir, config_path, &opts.cloud_options)?;
        } else {
            init_existing(options, &project_dir, config_path, &opts.cloud_options)?;
        }
    } else {
        if options.link {
            anyhow::bail!(
                "{CONFIG_FILE_DISPLAY_NAME} not found, unable to link an existing {BRANDING} \
                instance without an initialized project. To initialize \
                a project, run `{BRANDING_CLI_CMD}` command without `--link` flag"
            )
        } else {
            let dir = options
                .project_dir
                .clone()
                .unwrap_or_else(|| env::current_dir().unwrap());
            let config_path = dir.join(if cfg!(feature = "gel") {
                PROJECT_FILES[0]
            } else {
                PROJECT_FILES[1]
            });
            init_new(options, &dir, config_path, opts)?;
        }
    };
    Ok(())
}

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    #[command(flatten)]
    pub cloud_opts: CloudOptions,

    /// Explicitly set a root directory for the project
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Specify the desired EdgeDB server version
    #[arg(long)]
    pub server_version: Option<Query>,

    /// Specify whether the existing EdgeDB server instance
    /// should be linked with the project
    #[arg(long)]
    pub link: bool,

    /// Specify the EdgeDB server instance to be associated with the project
    #[arg(long)]
    pub server_instance: Option<InstanceName>,

    /// Specify the default database for the project to use on that instance
    #[arg(long, short = 'd')]
    pub database: Option<String>,

    /// Deprecated parameter, does nothing.
    #[arg(long, hide = true)]
    pub server_start_conf: Option<create::StartConf>,

    /// Skip running migrations
    ///
    /// There are two main use cases for this option:
    /// 1. With `--link` to connect to a datastore with existing data
    /// 2. To initialize a new instance but then restore using a dump
    #[arg(long)]
    pub no_migrations: bool,

    /// Initialize in in non-interactive mode (accepting all defaults)
    #[arg(long)]
    pub non_interactive: bool,
}

pub fn init_existing(
    options: &Command,
    project_dir: &Path,
    config_path: PathBuf,
    cloud_options: &crate::options::CloudOptions,
) -> anyhow::Result<project::ProjectInfo> {
    msg!(
        "Found `{}` in {}",
        config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        project_dir.display()
    );
    msg!("Initializing project...");

    let stash_dir = get_stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("Project is already initialized.");
    }

    let config = config::read(&config_path)?;
    let schema_dir = config.project.schema_dir;
    let schema_dir_path = project_dir.join(&schema_dir);
    let schema_dir_path = if schema_dir_path.exists() {
        fs::canonicalize(&schema_dir_path)
            .with_context(|| format!("failed to canonicalize dir {schema_dir_path:?}"))?
    } else {
        schema_dir_path
    };
    let schema_files = project::find_schema_files(&schema_dir_path)?;

    let ver_query = if let Some(sver) = &options.server_version {
        sver.clone()
    } else {
        config.instance.server_version
    };
    let mut client = CloudClient::new(cloud_options)?;
    let (name, exists) = ask_name(project_dir, options, &mut client)?;

    if exists {
        let mut inst = project::Handle::probe(&name, project_dir, &schema_dir, &client)?;
        let specific_version: &Specific = &inst.get_version()?.specific();
        inst.check_version(&ver_query);

        if matches!(name, InstanceName::Cloud { .. }) {
            if options.non_interactive {
                inst.database = Some(options.database.clone().unwrap_or(
                    get_default_branch_or_database(specific_version, project_dir),
                ));
            } else {
                inst.database = Some(ask_database_or_branch(
                    specific_version,
                    project_dir,
                    options,
                )?);
            }
        } else {
            inst.database.clone_from(&options.database);
        }
        return do_link(&inst, options, &stash_dir);
    }

    match &name {
        InstanceName::Cloud { org_slug, name } => {
            msg!("Checking {BRANDING_CLOUD} versions...");

            let ver = cloud::versions::get_version(&ver_query, &client)
                .with_context(|| "could not initialize project")?;
            ver::print_version_hint(&ver, &ver_query);
            let database = ask_database(project_dir, options)?;

            table::settings(&[
                ("Project directory", project_dir.display().to_string()),
                ("Project config", config_path.display().to_string()),
                (
                    &format!(
                        "Schema dir {}",
                        if schema_files {
                            "(non-empty)"
                        } else {
                            "(empty)"
                        }
                    ),
                    schema_dir_path.display().to_string(),
                ),
                (
                    if ver.major >= 5 {
                        "Branch name"
                    } else {
                        "Database name"
                    },
                    database.to_string(),
                ),
                ("Version", ver.to_string()),
                ("Instance name", name.to_string()),
            ]);

            if !schema_files {
                project::write_schema_default(&schema_dir, &Query::from_version(&ver)?)?;
            }
            do_cloud_init(
                name.to_owned(),
                org_slug.to_owned(),
                &stash_dir,
                project_dir,
                &schema_dir,
                &ver,
                &database,
                options,
                &client,
            )
        }
        InstanceName::Local(name) => {
            msg!("Checking {BRANDING} versions...");

            let pkg = repository::get_server_package(&ver_query)?.with_context(|| {
                format!(
                    "cannot find package matching {}. \
                    (Use `{BRANDING_CLI_CMD} server list-versions` to see all available)",
                    ver_query.display()
                )
            })?;
            let specific_version = &pkg.version.specific();
            ver::print_version_hint(specific_version, &ver_query);

            let mut branch: Option<String> = None;
            if !options.non_interactive && specific_version.major >= 5 {
                branch = Some(ask_branch()?);
            }

            let meth = if cfg!(windows) {
                "WSL".to_string()
            } else {
                "portable package".to_string()
            };

            let schema_dir_key = &format!(
                "Schema dir {}",
                if schema_files {
                    "(non-empty)"
                } else {
                    "(empty)"
                }
            );

            let mut rows: Vec<(&str, String)> = vec![
                ("Project directory", project_dir.display().to_string()),
                ("Project config", config_path.display().to_string()),
                (schema_dir_key, schema_dir_path.display().to_string()),
                ("Installation method", meth),
                ("Version", pkg.version.to_string()),
                ("Instance name", name.clone()),
            ];

            if let Some(branch) = branch.clone() {
                rows.push(("Branch", branch))
            }

            table::settings(rows.as_slice());

            if !schema_files {
                project::write_schema_default(
                    &schema_dir,
                    &Query::from_version(specific_version)?,
                )?;
            }

            do_init(
                name,
                &pkg,
                &stash_dir,
                project_dir,
                &schema_dir,
                &branch.unwrap_or(create::get_default_branch_name(specific_version)),
                options,
            )
        }
    }
}

fn do_init(
    name: &str,
    pkg: &PackageInfo,
    stash_dir: &Path,
    project_dir: &Path,
    schema_dir: &Path,
    database: &str,
    options: &Command,
) -> anyhow::Result<project::ProjectInfo> {
    let port = allocate_port(name)?;
    let paths = Paths::get(name)?;
    let inst_name = InstanceName::Local(name.to_owned());

    let instance = if cfg!(windows) {
        let q = repository::Query::from_version(&pkg.version.specific())?;
        windows::create_instance(
            &create::Command {
                name: Some(inst_name.clone()),
                nightly: false,
                channel: q.cli_channel(),
                version: q.version,
                cloud_params: CloudInstanceParams {
                    region: None,
                    billables: CloudInstanceBillables {
                        tier: None,
                        compute_size: None,
                        storage_size: None,
                    },
                },
                cloud_backup_source: create::CloudBackupSourceParams {
                    from_backup_id: None,
                    from_instance: None,
                },
                port: Some(port),
                start_conf: None,
                default_user: None,
                non_interactive: true,
                cloud_opts: options.cloud_opts.clone(),
                default_branch: Some(database.to_string()),
            },
            name,
            port,
            &paths,
        )?;
        create::create_service(&InstanceInfo {
            name: name.into(),
            installation: None,
            port,
        })?;
        project::InstanceKind::Wsl
    } else {
        let inst = install::package(pkg).context(concatcp!("error installing ", BRANDING))?;
        let version = inst.version.specific();
        let info = InstanceInfo {
            name: name.into(),
            installation: Some(inst),
            port,
        };
        create::bootstrap(
            &paths,
            &info,
            create::get_default_user_name(&version),
            database,
        )?;
        match create::create_service(&info) {
            Ok(()) => {}
            Err(e) => {
                log::warn!("Error running {BRANDING} as a service: {e:#}");
                print::warn!(
                    "{BRANDING} will not start on next login. \
                             Trying to start database in the background..."
                );
                control::start(&control::Start {
                    name: None,
                    instance: Some(inst_name.clone()),
                    foreground: false,
                    auto_restart: false,
                    managed_by: None,
                })?;
            }
        }
        project::InstanceKind::Portable(info)
    };

    let handle = project::Handle {
        name: name.into(),
        project_dir: project_dir.into(),
        schema_dir: schema_dir.into(),
        instance,
        database: options.database.clone(),
    };

    let mut stash = project::StashDir::new(project_dir, name);
    stash.database = handle.database.as_deref();
    stash.write(stash_dir)?;

    if !options.no_migrations {
        migrate(&handle, false)?;
    } else {
        create_database(&handle)?;
    }
    print_initialized(name, &options.project_dir);
    Ok(project::ProjectInfo {
        instance_name: name.into(),
        stash_dir: stash_dir.into(),
    })
}

fn do_cloud_init(
    name: String,
    org: String,
    stash_dir: &Path,
    project_dir: &Path,
    schema_dir: &Path,
    version: &ver::Specific,
    database: &str,
    options: &Command,
    client: &CloudClient,
) -> anyhow::Result<project::ProjectInfo> {
    let request = crate::cloud::ops::CloudInstanceCreate {
        name: name.clone(),
        org: org.clone(),
        version: version.to_string(),
        region: None,
        tier: None,
        requested_resources: None,
        source_instance_id: None,
        source_backup_id: None,
    };
    crate::cloud::ops::create_cloud_instance(client, &request)?;
    let full_name = format!("{org}/{name}");

    let handle = project::Handle {
        name: full_name.clone(),
        schema_dir: schema_dir.into(),
        instance: project::InstanceKind::Remote,
        project_dir: project_dir.into(),
        database: Some(database.to_owned()),
    };

    let mut stash = project::StashDir::new(project_dir, &full_name);
    stash.cloud_profile = client.profile.as_deref().or(Some("default"));
    stash.database = handle.database.as_deref();
    stash.write(stash_dir)?;

    if !options.no_migrations {
        migrate(&handle, false)?;
    } else {
        create_database(&handle)?;
    }
    print_initialized(&full_name, &options.project_dir);
    Ok(project::ProjectInfo {
        instance_name: full_name,
        stash_dir: stash_dir.into(),
    })
}

fn link(
    options: &Command,
    project_dir: &Path,
    config_path: PathBuf,
    cloud_options: &crate::options::CloudOptions,
) -> anyhow::Result<project::ProjectInfo> {
    msg!(
        "Found `{}` in {}",
        config_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        project_dir.display()
    );
    msg!("Linking project...");

    let stash_dir = get_stash_path(project_dir)?;
    if stash_dir.exists() {
        anyhow::bail!("Project is already linked");
    }

    let config = config::read(&config_path)?;
    let ver_query = config.instance.server_version;

    let mut client = CloudClient::new(cloud_options)?;
    let name = if let Some(name) = &options.server_instance {
        name.clone()
    } else if options.non_interactive {
        anyhow::bail!(
            "Existing instance name should be specified \
                       with `--server-instance` when linking project \
                       in non-interactive mode"
        )
    } else {
        ask_existing_instance_name(&mut client)?
    };
    let schema_dir = &config.project.schema_dir;
    let mut inst = project::Handle::probe(&name, project_dir, schema_dir, &client)?;
    if matches!(name, InstanceName::Cloud { .. }) {
        if options.non_interactive {
            inst.database = Some(
                options
                    .database
                    .clone()
                    .unwrap_or(directory_to_name(project_dir, "edgedb").to_owned()),
            )
        } else {
            inst.database = Some(ask_database(project_dir, options)?);
        }
    } else {
        inst.database.clone_from(&options.database);
    }
    inst.check_version(&ver_query);
    do_link(&inst, options, &stash_dir)
}

fn do_link(
    inst: &project::Handle,
    options: &Command,
    stash_dir: &Path,
) -> anyhow::Result<project::ProjectInfo> {
    let mut stash = project::StashDir::new(&inst.project_dir, &inst.name);
    if let project::InstanceKind::Cloud { cloud_client, .. } = inst.instance {
        let profile = cloud_client.profile.as_deref().unwrap_or("default");
        stash.cloud_profile = Some(profile);
    };
    stash.database = inst.database.as_deref();
    stash.write(stash_dir)?;

    if !options.no_migrations {
        migrate(inst, !options.non_interactive)?;
    } else {
        create_database(inst)?;
    }

    print::success!("Project linked");
    if let Some(dir) = &options.project_dir {
        eprintln!(
            "To connect to {}, navigate to {} and run `{BRANDING_CLI_CMD}`",
            inst.name,
            dir.display()
        );
    } else {
        eprintln!("To connect to {}, run `{BRANDING_CLI_CMD}`", inst.name);
    }

    Ok(project::ProjectInfo {
        instance_name: inst.name.clone(),
        stash_dir: stash_dir.into(),
    })
}

fn directory_to_name(path: &Path, default: &str) -> String {
    let path_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(default);
    let stem = path_stem.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    let stem = stem.trim_matches('_');
    if stem.is_empty() {
        default.into()
    } else {
        stem.into()
    }
}

fn init_new(
    options: &Command,
    project_dir: &Path,
    config_path: PathBuf,
    opts: &crate::options::Options,
) -> anyhow::Result<project::ProjectInfo> {
    eprintln!(
        "No {CONFIG_FILE_DISPLAY_NAME} found in `{}` or above",
        project_dir.display()
    );

    let stash_dir = get_stash_path(project_dir)?;
    if stash_dir.exists() {
        anyhow::bail!(
            "{CONFIG_FILE_DISPLAY_NAME} deleted after \
                       project initialization. \
                       Please run `{BRANDING_CLI_CMD} project unlink -D` to \
                       clean up old database instance."
        );
    }

    if options.non_interactive {
        eprintln!("Initializing new project...");
    } else {
        let mut q = question::Confirm::new("Do you want to initialize a new project?");
        q.default(true);
        if !q.ask()? {
            return Err(ExitCode::new(0).into());
        }
    }

    let schema_dir = Path::new("dbschema");
    let schema_dir_path = project_dir.join(schema_dir);
    let schema_files = project::find_schema_files(schema_dir)?;

    let mut client = CloudClient::new(&opts.cloud_options)?;
    let (inst_name, exists) = ask_name(project_dir, options, &mut client)?;

    if exists {
        let mut inst;
        inst = project::Handle::probe(&inst_name, project_dir, schema_dir, &client)?;
        let specific_version: &Specific = &inst.get_version()?.specific();
        let version_query = Query::from_version(specific_version)?;
        project::write_config(&config_path, &version_query)?;
        if !schema_files {
            project::write_schema_default(&schema_dir_path, &version_query)?;
        }
        if matches!(inst_name, InstanceName::Cloud { .. }) {
            if options.non_interactive {
                inst.database = Some(options.database.clone().unwrap_or(
                    get_default_branch_or_database(specific_version, project_dir),
                ));
            } else {
                inst.database = Some(ask_database_or_branch(
                    specific_version,
                    project_dir,
                    options,
                )?);
            }
        } else {
            inst.database.clone_from(&options.database);
        }
        return do_link(&inst, options, &stash_dir);
    };

    match &inst_name {
        InstanceName::Cloud { org_slug, name } => {
            msg!("Checking {BRANDING_CLOUD} versions...");
            client.ensure_authenticated()?;

            let (ver_query, version) = ask_cloud_version(options, &client)?;
            ver::print_version_hint(&version, &ver_query);
            let database = ask_database_or_branch(&version, project_dir, options)?;
            table::settings(&[
                ("Project directory", project_dir.display().to_string()),
                ("Project config", config_path.display().to_string()),
                (
                    &format!(
                        "Schema dir {}",
                        if schema_files {
                            "(non-empty)"
                        } else {
                            "(empty)"
                        }
                    ),
                    schema_dir_path.display().to_string(),
                ),
                (
                    if version.major >= 5 {
                        "Branch"
                    } else {
                        "Database"
                    },
                    database.to_string(),
                ),
                ("Version", version.to_string()),
                ("Instance name", name.clone()),
            ]);
            project::write_config(&config_path, &ver_query)?;
            if !schema_files {
                project::write_schema_default(&schema_dir_path, &Query::from_version(&version)?)?;
            }

            do_cloud_init(
                name.to_owned(),
                org_slug.to_owned(),
                &stash_dir,
                project_dir,
                schema_dir,
                &version,
                &database,
                options,
                &client,
            )
        }
        InstanceName::Local(name) => {
            msg!("Checking {BRANDING} versions...");
            let (ver_query, pkg) = ask_local_version(options)?;
            let specific_version = &pkg.version.specific();
            ver::print_version_hint(specific_version, &ver_query);

            let mut branch: Option<String> = None;
            if !options.non_interactive && specific_version.major >= 5 {
                branch = Some(ask_branch()?);
            }

            let meth = if cfg!(windows) {
                "WSL".to_string()
            } else {
                "portable package".to_string()
            };

            let schema_dir_key = &format!(
                "Schema dir {}",
                if schema_files {
                    "(non-empty)"
                } else {
                    "(empty)"
                }
            );

            let mut rows: Vec<(&str, String)> = vec![
                ("Project directory", project_dir.display().to_string()),
                ("Project config", config_path.display().to_string()),
                (schema_dir_key, schema_dir_path.display().to_string()),
                ("Installation method", meth),
                ("Version", pkg.version.to_string()),
                ("Instance name", name.clone()),
            ];

            if let Some(branch) = branch.clone() {
                rows.push(("Branch", branch))
            }

            table::settings(rows.as_slice());

            project::write_config(&config_path, &ver_query)?;
            if !schema_files {
                project::write_schema_default(
                    &schema_dir_path,
                    &Query::from_version(specific_version)?,
                )?;
            }

            do_init(
                name,
                &pkg,
                &stash_dir,
                project_dir,
                schema_dir,
                &branch.unwrap_or(create::get_default_branch_name(specific_version)),
                options,
            )
        }
    }
}

fn ask_name(
    dir: &Path,
    options: &Command,
    cloud_client: &mut CloudClient,
) -> anyhow::Result<(InstanceName, bool)> {
    let instances = credentials::all_instance_names()?;
    let default_name = if let Some(name) = &options.server_instance {
        name.clone()
    } else {
        let base_name = directory_to_name(dir, "instance");
        let mut name = base_name.clone();

        while instances.contains(&name) {
            name = format!("{}_{:04}", base_name, thread_rng().gen_range(0..10000));
        }
        InstanceName::Local(name)
    };
    if options.non_interactive {
        let exists = match &default_name {
            InstanceName::Local(name) => instances.contains(name),
            InstanceName::Cloud { org_slug, name } => {
                cloud_client.ensure_authenticated()?;
                let inst =
                    crate::cloud::ops::find_cloud_instance_by_name(name, org_slug, cloud_client)?;
                inst.is_some()
            }
        };
        if exists {
            anyhow::bail!(format!(
                "Instance {:?} already exists, \
                               to link project with it pass `--link` \
                               flag explicitly",
                default_name.to_string()
            ))
        }
        return Ok((default_name, false));
    }
    let mut q = question::String::new(concatcp!(
        "Specify the name of the ",
        BRANDING,
        " instance to use with this project"
    ));
    let default_name_str = default_name.to_string();
    q.default(&default_name_str);
    loop {
        let default_name_clone = default_name.clone();
        let mut q = question::String::new(concatcp!(
            "Specify the name of the ",
            BRANDING,
            " instance to use with this project"
        ));
        let default_name_str = default_name_clone.to_string();
        let target_name = q.default(&default_name_str).ask()?;
        let inst_name = match InstanceName::from_str(&target_name) {
            Ok(name) => name,
            Err(e) => {
                print::error!("{e}");
                continue;
            }
        };
        let exists = match &inst_name {
            InstanceName::Local(name) => instances.contains(name),
            InstanceName::Cloud { org_slug, name } => {
                if !cloud_client.is_logged_in {
                    if let Err(e) = crate::cloud::ops::prompt_cloud_login(cloud_client) {
                        print::error!("{e}");
                        continue;
                    }
                }
                crate::cloud::ops::find_cloud_instance_by_name(name, org_slug, cloud_client)?
                    .is_some()
            }
        };
        if exists {
            let confirm = question::Confirm::new(format!(
                "Do you want to use existing instance {target_name:?} \
                         for the project?"
            ));
            if confirm.ask()? {
                return Ok((inst_name, true));
            }
        } else {
            return Ok((inst_name, false));
        }
    }
}

fn get_default_branch_or_database(version: &Specific, project_dir: &Path) -> String {
    if version.major >= 5 {
        return String::from("main");
    }

    directory_to_name(project_dir, "edgedb")
}

fn ask_database_or_branch(
    version: &Specific,
    project_dir: &Path,
    options: &Command,
) -> anyhow::Result<String> {
    if version.major >= 5 {
        return ask_branch();
    }

    ask_database(project_dir, options)
}

fn ask_database(project_dir: &Path, options: &Command) -> anyhow::Result<String> {
    if let Some(name) = &options.database {
        return Ok(name.clone());
    }
    let default = directory_to_name(project_dir, "edgedb");
    let mut q = question::String::new("Specify database name:");
    q.default(&default);
    loop {
        let name = q.ask()?;
        if name.trim().is_empty() {
            print::error!("Non-empty name is required");
        } else {
            return Ok(name.trim().into());
        }
    }
}

fn ask_branch() -> anyhow::Result<String> {
    let mut q = question::String::new("Specify branch name:");
    q.default("main");
    loop {
        let name = q.ask()?;
        if name.trim().is_empty() {
            print::error!("Non-empty name is required");
        } else {
            return Ok(name.trim().into());
        }
    }
}

fn ask_local_version(options: &Command) -> anyhow::Result<(Query, PackageInfo)> {
    let ver_query = options.server_version.clone().unwrap_or(Query::stable());
    if options.non_interactive || options.server_version.is_some() {
        let pkg = repository::get_server_package(&ver_query)?
            .with_context(|| format!("no package matching {} found", ver_query.display()))?;
        if options.server_version.is_some() {
            return Ok((ver_query, pkg));
        } else {
            return Ok((Query::from_version(&pkg.version.specific())?, pkg));
        }
    }
    let default = repository::get_server_package(&ver_query)?;
    let default_ver = if let Some(pkg) = &default {
        Query::from_version(&pkg.version.specific())?.as_config_value()
    } else {
        String::new()
    };
    let mut q = question::String::new(concatcp!(
        "Specify the version of the ",
        BRANDING,
        " instance to use with this project"
    ));
    q.default(&default_ver);
    loop {
        let value = q.ask()?;
        let value = value.trim();
        if value == "nightly" {
            match repository::get_server_package(&Query::nightly()) {
                Ok(Some(pkg)) => return Ok((Query::nightly(), pkg)),
                Ok(None) => {
                    print::error!("No nightly versions found");
                    continue;
                }
                Err(e) => {
                    print::error!("Cannot find nightly version: {e}");
                    continue;
                }
            }
        } else if value == "testing" {
            match repository::get_server_package(&Query::testing()) {
                Ok(Some(pkg)) => return Ok((Query::testing(), pkg)),
                Ok(None) => {
                    print::error!("No testing versions found");
                    continue;
                }
                Err(e) => {
                    print::error!("Cannot find testing version: {e}");
                    continue;
                }
            }
        } else {
            match parse_ver_and_find(value) {
                Ok(Some(pair)) => return Ok(pair),
                Ok(None) => {
                    print::error!("No matching packages found");
                    print_versions("Available versions")?;
                    continue;
                }
                Err(e) => {
                    print::error!("{e}");
                    print_versions("Available versions")?;
                    continue;
                }
            }
        }
    }
}

fn ask_existing_instance_name(cloud_client: &mut CloudClient) -> anyhow::Result<InstanceName> {
    let instances = credentials::all_instance_names()?;

    loop {
        let mut q = question::String::new(concatcp!(
            "Specify the name of the ",
            BRANDING,
            " instance to link with this project"
        ));
        let target_name = q.ask()?;

        let inst_name = match InstanceName::from_str(&target_name) {
            Ok(name) => name,
            Err(e) => {
                print::error!("{e}");
                continue;
            }
        };
        let exists = match &inst_name {
            InstanceName::Local(name) => instances.contains(name),
            InstanceName::Cloud { org_slug, name } => {
                if !cloud_client.is_logged_in {
                    if let Err(e) = crate::cloud::ops::prompt_cloud_login(cloud_client) {
                        print::error!("{e}");
                        continue;
                    }
                }
                crate::cloud::ops::find_cloud_instance_by_name(name, org_slug, cloud_client)?
                    .is_some()
            }
        };
        if exists {
            return Ok(inst_name);
        } else {
            print::error!("Instance {target_name:?} does not exist");
        }
    }
}

fn parse_ver_and_find(value: &str) -> anyhow::Result<Option<(Query, PackageInfo)>> {
    let filter = value.parse()?;
    let query = Query::from_filter(&filter)?;
    if let Some(pkg) = repository::get_server_package(&query)? {
        Ok(Some((query, pkg)))
    } else {
        Ok(None)
    }
}

fn print_versions(title: &str) -> anyhow::Result<()> {
    let mut avail = repository::get_server_packages(Channel::Stable)?;
    avail.sort_by(|a, b| b.version.cmp(&a.version));
    println!(
        "{}: {}{}",
        title,
        avail
            .iter()
            .filter_map(|p| Query::from_version(&p.version.specific()).ok())
            .take(5)
            .map(|v| v.as_config_value())
            .collect::<Vec<_>>()
            .join(", "),
        if avail.len() > 5 { " ..." } else { "" },
    );
    Ok(())
}

fn parse_ver_and_find_cloud(
    value: &str,
    client: &CloudClient,
) -> anyhow::Result<(Query, ver::Specific)> {
    let filter = value.parse()?;
    let query = Query::from_filter(&filter)?;
    let version = cloud::versions::get_version(&query, client)?;
    Ok((query, version))
}

fn ask_cloud_version(
    options: &Command,
    client: &CloudClient,
) -> anyhow::Result<(Query, ver::Specific)> {
    let ver_query = options.server_version.clone().unwrap_or(Query::stable());
    if options.non_interactive || options.server_version.is_some() {
        let version = cloud::versions::get_version(&ver_query, client)?;
        return Ok((ver_query, version));
    }
    let default = cloud::versions::get_version(&Query::stable(), client)?;
    let default_ver = Query::from_version(&default)?.as_config_value();
    let mut q = question::String::new(concatcp!(
        "Specify the version of the ",
        BRANDING,
        " instance to use with this project"
    ));
    q.default(&default_ver);
    loop {
        let value = q.ask()?;
        let value = value.trim();
        if value == "nightly" {
            match cloud::versions::get_version(&Query::nightly(), client) {
                Ok(v) => return Ok((Query::nightly(), v)),
                Err(e) => {
                    print::error!("{e}");
                    continue;
                }
            }
        } else if value == "testing" {
            match cloud::versions::get_version(&Query::testing(), client) {
                Ok(v) => return Ok((Query::testing(), v)),
                Err(e) => {
                    print::error!("{e}");
                    continue;
                }
            }
        } else {
            match parse_ver_and_find_cloud(value, client) {
                Ok(pair) => return Ok(pair),
                Err(e) => {
                    print::error!("{e}");
                    print_cloud_versions("Available versions", client)?;
                    continue;
                }
            }
        }
    }
}

fn print_cloud_versions(title: &str, client: &CloudClient) -> anyhow::Result<()> {
    let mut avail: Vec<ver::Specific> = cloud::ops::get_versions(client)?
        .into_iter()
        .map(|v| v.version.parse::<ver::Specific>().unwrap())
        .collect();
    avail.sort();
    println!(
        "{}: {}{}",
        title,
        avail
            .iter()
            .filter_map(|p| Query::from_version(p).ok())
            .take(5)
            .map(|v| v.as_config_value())
            .collect::<Vec<_>>()
            .join(", "),
        if avail.len() > 5 { " ..." } else { "" },
    );
    Ok(())
}

fn print_initialized(name: &str, dir_option: &Option<PathBuf>) {
    print::success!("Project initialized.");
    if let Some(dir) = dir_option {
        msg!(
            "To connect to {}, navigate to {} and run `{}`",
            name.emphasize(),
            dir.display(),
            BRANDING_CLI_CMD
        );
    } else {
        msg!(
            "To connect to {}, run `{}`",
            name.emphasize(),
            BRANDING_CLI_CMD
        );
    }
}

#[tokio::main(flavor = "current_thread")]
async fn create_database(inst: &project::Handle<'_>) -> anyhow::Result<()> {
    create_database_async(inst).await
}

async fn create_database_async(inst: &project::Handle<'_>) -> anyhow::Result<()> {
    let Some(name) = &inst.database else {
        return Ok(());
    };
    let config = inst.get_default_builder()?.build_env().await?;
    if name == config.database() {
        return Ok(());
    }
    let mut conn = Connection::connect(&config, QUERY_TAG).await?;
    ensure_database(&mut conn, name).await?;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn migrate(inst: &project::Handle<'_>, ask_for_running: bool) -> anyhow::Result<()> {
    migrate_async(inst, ask_for_running).await
}

async fn migrate_async(inst: &project::Handle<'_>, ask_for_running: bool) -> anyhow::Result<()> {
    use crate::commands::Options;
    use crate::migrations::options::{Migrate, MigrationConfig};
    use Action::*;

    #[derive(Clone, Copy)]
    enum Action {
        Retry,
        Service,
        Run,
        Skip,
    }

    msg!("Applying migrations...");

    let mut conn = loop {
        match inst.get_default_connection().await {
            Ok(conn) => break conn,
            Err(e) if ask_for_running && inst.instance.is_local() => {
                print::error!("{e}");
                let mut q = question::Numeric::new(format!(
                    "Cannot connect to instance {:?}. Options:",
                    inst.name,
                ));
                q.option("Start the service (if possible).", Service);
                q.option(
                    "Start in the foreground, \
                          apply migrations and shut down.",
                    Run,
                );
                q.option("Instance has been started manually, retry connect", Retry);
                q.option("Skip migrations.", Skip);
                match q.async_ask().await? {
                    Service => match start(inst) {
                        Ok(()) => continue,
                        Err(e) => {
                            print::error!("{e}");
                            continue;
                        }
                    },
                    Run => {
                        run_and_migrate(inst)?;
                        return Ok(());
                    }
                    Retry => continue,
                    Skip => {
                        print::warn!("Skipping migrations.");
                        msg!(
                            "You can use `{BRANDING_CLI_CMD} migrate` to apply migrations \
                               once the service is up and running."
                        );
                        return Ok(());
                    }
                }
            }
            Err(e) => return Err(e)?,
        };
    };
    if let Some(database) = &inst.database {
        ensure_database(&mut conn, database).await?;
        conn = inst.get_connection().await?;
    }

    migrations::migrate(
        &mut conn,
        &Options {
            command_line: true,
            styler: None,
            conn_params: Connector::new(inst.get_builder()?.build_env().await.map_err(Into::into)),
        },
        &Migrate {
            cfg: MigrationConfig {
                schema_dir: Some(inst.project_dir.join(&inst.schema_dir)),
            },
            quiet: false,
            to_revision: None,
            dev_mode: false,
            single_transaction: false,
            conn: None,
        },
    )
    .await?;
    Ok(())
}

fn run_and_migrate(info: &project::Handle) -> anyhow::Result<()> {
    match &info.instance {
        project::InstanceKind::Portable(inst) => {
            control::ensure_runstate_dir(&info.name)?;
            let mut cmd = control::get_server_cmd(inst, false)?;
            cmd.background_for(|| Ok(migrate_async(info, false)))?;
            Ok(())
        }
        project::InstanceKind::Wsl => {
            let mut cmd = windows::server_cmd(&info.name, false)?;
            cmd.background_for(|| Ok(migrate_async(info, false)))?;
            Ok(())
        }
        project::InstanceKind::Remote => {
            anyhow::bail!(
                "remote instance not running, \
                          cannot run migrations"
            );
        }
        project::InstanceKind::Cloud { .. } => todo!(),
    }
}

async fn ensure_database(cli: &mut Connection, name: &str) -> anyhow::Result<()> {
    let name = quote_name(name);
    match cli.execute(&format!("CREATE DATABASE {name}"), &()).await {
        Ok(_) => Ok(()),
        Err(e) if e.is::<DuplicateDatabaseDefinitionError>() => Ok(()),
        Err(e) => Err(e)?,
    }
}

fn start(handle: &project::Handle) -> anyhow::Result<()> {
    match &handle.instance {
        project::InstanceKind::Portable(inst) => {
            control::do_start(inst)?;
            Ok(())
        }
        project::InstanceKind::Wsl => {
            windows::daemon_start(&handle.name)?;
            Ok(())
        }
        project::InstanceKind::Remote => {
            anyhow::bail!(
                "remote instance not running, \
                          cannot run migrations"
            );
        }
        project::InstanceKind::Cloud { .. } => todo!(),
    }
}
