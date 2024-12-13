use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Context;
use clap::ValueHint;
use const_format::concatcp;
use edgedb_tokio::get_project_path;
use edgedb_tokio::get_stash_path;
use edgedb_tokio::PROJECT_FILES;
use fn_error_context::context;
use rand::{thread_rng, Rng};

use edgedb_errors::DuplicateDatabaseDefinitionError;
use edgedb_tokio::Builder;
use edgeql_parser::helpers::quote_name;

use crate::branding::BRANDING_CLOUD;
use crate::branding::QUERY_TAG;
use crate::branding::{
    BRANDING, BRANDING_CLI_CMD, BRANDING_SCHEMA_FILE_EXT, CONFIG_FILE_DISPLAY_NAME,
};
use crate::branding::{BRANDING_DEFAULT_USERNAME, BRANDING_DEFAULT_USERNAME_LEGACY};
use crate::cloud;
use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::options::CloudOptions;
use crate::platform::{bytes_to_path, path_bytes};
use crate::platform::{config_dir, is_schema_file, symlink_dir, tmp_file_path};
use crate::portable::config;
use crate::portable::control;
use crate::portable::create;
use crate::portable::destroy;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{allocate_port, InstanceInfo, Paths};
use crate::portable::options::{self, InstanceName, Start, StartConf};
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{self, Channel, PackageInfo, Query};
use crate::portable::upgrade;
use crate::portable::ver;
use crate::portable::ver::Specific;
use crate::portable::windows;
use crate::print::{self, msg, Highlight};

use crate::question;
use crate::table;

const DEFAULT_SCHEMA: &str = "\
    module default {\n\
    \n\
    }\n\
";

const FUTURES_SCHEMA: &str = "\
    # Disable the application of access policies within access policies\n\
    # themselves. This behavior will become the default in EdgeDB 3.0.\n\
    # See: https://www.edgedb.com/docs/reference/ddl/access_policies#nonrecursive\n\
    using future nonrecursive_access_policies;\n\
";

const SIMPLE_SCOPING_SCHEMA: &str = "\
    # Use a simpler algorithm for resolving the scope of object names.\n\
    # This behavior will become the default in Gel 7.0.\n\
    # See: https://docs.edgedb.com/database/edgeql/path_resolution#new-path-scoping\n\
    using future simple_scoping;\n\
";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectInfo {
    instance_name: String,
    stash_dir: PathBuf,
}

#[derive(clap::Args, Debug, Clone)]
#[command(version = "help_expand")]
#[command(disable_version_flag = true)]
pub struct ProjectCommand {
    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    /// Initialize project or link to existing unlinked project
    Init(Init),
    /// Clean up project configuration.
    ///
    /// Use [`BRANDING_CLI_CMD`] project init to relink.
    Unlink(Unlink),
    /// Get various metadata about project instance
    Info(Info),
    /// Upgrade [`BRANDING`] instance used for current project
    ///
    /// Data is preserved using a dump/restore mechanism.
    ///
    /// Upgrades to version specified in `{gel,edgedb}.toml` unless other options specified.
    ///
    /// Note: May fail if lower version is specified (e.g. moving from nightly to stable).
    Upgrade(Upgrade),
}

#[derive(clap::Args, Debug, Clone)]
pub struct Init {
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
    pub server_start_conf: Option<StartConf>,

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

#[derive(clap::Args, Debug, Clone)]
pub struct Unlink {
    #[command(flatten)]
    pub cloud_opts: CloudOptions,

    /// Explicitly set a root directory for the project
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// If specified, the associated EdgeDB instance is destroyed
    /// using `edgedb instance destroy`.
    #[arg(long, short = 'D')]
    pub destroy_server_instance: bool,

    /// Unlink in in non-interactive mode (accepting all defaults)
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Info {
    /// Explicitly set a root directory for the project
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Display only the instance name (shortcut to `--get instance-name`)
    #[arg(long)]
    pub instance_name: bool,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    #[arg(long, value_parser=[
        "instance-name",
        "cloud-profile",
    ])]
    /// Get a specific value:
    ///
    /// * `instance-name` -- Name of the listance the project is linked to
    pub get: Option<String>,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Upgrade {
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

pub struct Handle<'a> {
    name: String,
    instance: InstanceKind<'a>,
    project_dir: PathBuf,
    schema_dir: PathBuf,
    database: Option<String>,
}

pub struct StashDir<'a> {
    project_dir: &'a Path,
    instance_name: &'a str,
    database: Option<&'a str>,
    cloud_profile: Option<&'a str>,
}

pub struct WslInfo {}

pub enum InstanceKind<'a> {
    Remote,
    Portable(InstanceInfo),
    Wsl(WslInfo),
    Cloud {
        org_slug: String,
        name: String,
        cloud_client: &'a CloudClient,
    },
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct JsonInfo<'a> {
    instance_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_profile: Option<&'a str>,
    root: &'a Path,
}

pub fn init(options: &Init, opts: &crate::options::Options) -> anyhow::Result<()> {
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

    let Some((project_dir, config_path)) = project_dir(options.project_dir.as_deref())? else {
        if options.link {
            anyhow::bail!(
                "{CONFIG_FILE_DISPLAY_NAME} not found, unable to link an existing {BRANDING} \
                instance without an initialized project. To initialize \
                a project, run `{BRANDING_CLI_CMD}` command without `--link` flag"
            )
        }
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
        return Ok(());
    };

    if options.link {
        link(options, &project_dir, config_path, &opts.cloud_options)?;
    } else {
        init_existing(options, &project_dir, config_path, &opts.cloud_options)?;
    }
    Ok(())
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

fn ask_database(project_dir: &Path, options: &Init) -> anyhow::Result<String> {
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

fn ask_database_or_branch(
    version: &Specific,
    project_dir: &Path,
    options: &Init,
) -> anyhow::Result<String> {
    if version.major >= 5 {
        return ask_branch();
    }

    ask_database(project_dir, options)
}

pub fn get_default_branch_name(version: &Specific) -> String {
    if version.major >= 5 {
        return String::from("main");
    }

    String::from("edgedb")
}

pub fn get_default_user_name(version: &Specific) -> &'static str {
    if version.major >= 6 {
        BRANDING_DEFAULT_USERNAME
    } else {
        BRANDING_DEFAULT_USERNAME_LEGACY
    }
}

pub fn get_default_branch_or_database(version: &Specific, project_dir: &Path) -> String {
    if version.major >= 5 {
        return String::from("main");
    }

    directory_to_name(project_dir, "edgedb")
}

fn link(
    options: &Init,
    project_dir: &Path,
    config_path: PathBuf,
    cloud_options: &crate::options::CloudOptions,
) -> anyhow::Result<ProjectInfo> {
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
    let ver_query = config.edgedb.server_version;

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
    let mut inst = Handle::probe(&name, project_dir, schema_dir, &client)?;
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

fn do_link(inst: &Handle, options: &Init, stash_dir: &Path) -> anyhow::Result<ProjectInfo> {
    let mut stash = StashDir::new(&inst.project_dir, &inst.name);
    if let InstanceKind::Cloud { cloud_client, .. } = inst.instance {
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

    Ok(ProjectInfo {
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

fn ask_name(
    dir: &Path,
    options: &Init,
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

pub fn init_existing(
    options: &Init,
    project_dir: &Path,
    config_path: PathBuf,
    cloud_options: &crate::options::CloudOptions,
) -> anyhow::Result<ProjectInfo> {
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
    let schema_files = find_schema_files(&schema_dir_path)?;

    let ver_query = if let Some(sver) = &options.server_version {
        sver.clone()
    } else {
        config.edgedb.server_version
    };
    let mut client = CloudClient::new(cloud_options)?;
    let (name, exists) = ask_name(project_dir, options, &mut client)?;

    if exists {
        let mut inst = Handle::probe(&name, project_dir, &schema_dir, &client)?;
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
                write_schema_default(&schema_dir, &Query::from_version(&ver)?)?;
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
                write_schema_default(&schema_dir, &Query::from_version(specific_version)?)?;
            }

            do_init(
                name,
                &pkg,
                &stash_dir,
                project_dir,
                &schema_dir,
                &branch.unwrap_or(get_default_branch_name(specific_version)),
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
    options: &Init,
) -> anyhow::Result<ProjectInfo> {
    let port = allocate_port(name)?;
    let paths = Paths::get(name)?;
    let inst_name = InstanceName::Local(name.to_owned());

    let instance = if cfg!(windows) {
        let q = repository::Query::from_version(&pkg.version.specific())?;
        windows::create_instance(
            &options::Create {
                name: Some(inst_name.clone()),
                nightly: false,
                channel: q.cli_channel(),
                version: q.version,
                cloud_params: options::CloudInstanceParams {
                    region: None,
                    billables: options::CloudInstanceBillables {
                        tier: None,
                        compute_size: None,
                        storage_size: None,
                    },
                },
                cloud_backup_source: options::CloudBackupSourceParams {
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
        InstanceKind::Wsl(WslInfo {})
    } else {
        let inst = install::package(pkg).context(concatcp!("error installing ", BRANDING))?;
        let version = inst.version.specific();
        let info = InstanceInfo {
            name: name.into(),
            installation: Some(inst),
            port,
        };
        create::bootstrap(&paths, &info, get_default_user_name(&version), database)?;
        match create::create_service(&info) {
            Ok(()) => {}
            Err(e) => {
                log::warn!("Error running {BRANDING} as a service: {e:#}");
                print::warn!(
                    "{BRANDING} will not start on next login. \
                             Trying to start database in the background..."
                );
                control::start(&Start {
                    name: None,
                    instance: Some(inst_name.clone()),
                    foreground: false,
                    auto_restart: false,
                    managed_by: None,
                })?;
            }
        }
        InstanceKind::Portable(info)
    };

    let handle = Handle {
        name: name.into(),
        project_dir: project_dir.into(),
        schema_dir: schema_dir.into(),
        instance,
        database: options.database.clone(),
    };

    let mut stash = StashDir::new(project_dir, name);
    stash.database = handle.database.as_deref();
    stash.write(stash_dir)?;

    if !options.no_migrations {
        migrate(&handle, false)?;
    } else {
        create_database(&handle)?;
    }
    print_initialized(name, &options.project_dir);
    Ok(ProjectInfo {
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
    options: &Init,
    client: &CloudClient,
) -> anyhow::Result<ProjectInfo> {
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

    let handle = Handle {
        name: full_name.clone(),
        schema_dir: schema_dir.into(),
        instance: InstanceKind::Remote,
        project_dir: project_dir.into(),
        database: Some(database.to_owned()),
    };

    let mut stash = StashDir::new(project_dir, &full_name);
    stash.cloud_profile = client.profile.as_deref().or(Some("default"));
    stash.database = handle.database.as_deref();
    stash.write(stash_dir)?;

    if !options.no_migrations {
        migrate(&handle, false)?;
    } else {
        create_database(&handle)?;
    }
    print_initialized(&full_name, &options.project_dir);
    Ok(ProjectInfo {
        instance_name: full_name,
        stash_dir: stash_dir.into(),
    })
}

pub fn init_new(
    options: &Init,
    project_dir: &Path,
    config_path: PathBuf,
    opts: &crate::options::Options,
) -> anyhow::Result<ProjectInfo> {
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
    let schema_files = find_schema_files(schema_dir)?;

    let mut client = CloudClient::new(&opts.cloud_options)?;
    let (inst_name, exists) = ask_name(project_dir, options, &mut client)?;

    if exists {
        let mut inst;
        inst = Handle::probe(&inst_name, project_dir, schema_dir, &client)?;
        let specific_version: &Specific = &inst.get_version()?.specific();
        let version_query = Query::from_version(specific_version)?;
        write_config(&config_path, &version_query)?;
        if !schema_files {
            write_schema_default(&schema_dir_path, &version_query)?;
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
            write_config(&config_path, &ver_query)?;
            if !schema_files {
                write_schema_default(&schema_dir_path, &Query::from_version(&version)?)?;
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

            write_config(&config_path, &ver_query)?;
            if !schema_files {
                write_schema_default(&schema_dir_path, &Query::from_version(specific_version)?)?;
            }

            do_init(
                name,
                &pkg,
                &stash_dir,
                project_dir,
                schema_dir,
                &branch.unwrap_or(get_default_branch_name(specific_version)),
                options,
            )
        }
    }
}

pub fn stash_base() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

fn run_and_migrate(info: &Handle) -> anyhow::Result<()> {
    match &info.instance {
        InstanceKind::Portable(inst) => {
            control::ensure_runstate_dir(&info.name)?;
            let mut cmd = control::get_server_cmd(inst, false)?;
            cmd.background_for(|| Ok(migrate_async(info, false)))?;
            Ok(())
        }
        InstanceKind::Wsl(_) => {
            let mut cmd = windows::server_cmd(&info.name, false)?;
            cmd.background_for(|| Ok(migrate_async(info, false)))?;
            Ok(())
        }
        InstanceKind::Remote => {
            anyhow::bail!(
                "remote instance not running, \
                          cannot run migrations"
            );
        }
        InstanceKind::Cloud { .. } => todo!(),
    }
}

fn start(handle: &Handle) -> anyhow::Result<()> {
    match &handle.instance {
        InstanceKind::Portable(inst) => {
            control::do_start(inst)?;
            Ok(())
        }
        InstanceKind::Wsl(_) => {
            windows::daemon_start(&handle.name)?;
            Ok(())
        }
        InstanceKind::Remote => {
            anyhow::bail!(
                "remote instance not running, \
                          cannot run migrations"
            );
        }
        InstanceKind::Cloud { .. } => todo!(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn create_database(inst: &Handle<'_>) -> anyhow::Result<()> {
    create_database_async(inst).await
}

async fn ensure_database(cli: &mut Connection, name: &str) -> anyhow::Result<()> {
    let name = quote_name(name);
    match cli.execute(&format!("CREATE DATABASE {name}"), &()).await {
        Ok(_) => Ok(()),
        Err(e) if e.is::<DuplicateDatabaseDefinitionError>() => Ok(()),
        Err(e) => Err(e)?,
    }
}

async fn create_database_async(inst: &Handle<'_>) -> anyhow::Result<()> {
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
async fn migrate(inst: &Handle<'_>, ask_for_running: bool) -> anyhow::Result<()> {
    migrate_async(inst, ask_for_running).await
}

async fn migrate_async(inst: &Handle<'_>, ask_for_running: bool) -> anyhow::Result<()> {
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

impl<'a> StashDir<'a> {
    fn new(project_dir: &'a Path, instance_name: &'a str) -> StashDir<'a> {
        StashDir {
            project_dir,
            instance_name,
            database: None,
            cloud_profile: None,
        }
    }
    #[context("error writing project dir {:?}", dir)]
    fn write(&self, dir: &Path) -> anyhow::Result<()> {
        let tmp = tmp_file_path(dir);
        fs::create_dir_all(&tmp)?;
        fs::write(tmp.join("project-path"), path_bytes(self.project_dir)?)?;
        fs::write(tmp.join("instance-name"), self.instance_name.as_bytes())?;
        if let Some(profile) = self.cloud_profile {
            fs::write(tmp.join("cloud-profile"), profile.as_bytes())?;
        }
        if let Some(database) = &self.database {
            fs::write(tmp.join("database"), database.as_bytes())?;
        }

        let lnk = tmp.join("project-link");
        symlink_dir(self.project_dir, &lnk)
            .map_err(|e| {
                log::info!("Error symlinking project at {:?}: {}", lnk, e);
            })
            .ok();
        fs::rename(&tmp, dir)?;
        Ok(())
    }
}

impl InstanceKind<'_> {
    fn is_local(&self) -> bool {
        match self {
            InstanceKind::Wsl(_) => true,
            InstanceKind::Portable(_) => true,
            InstanceKind::Remote => false,
            InstanceKind::Cloud { .. } => false,
        }
    }
}

impl Handle<'_> {
    pub fn probe<'a>(
        name: &InstanceName,
        project_dir: &Path,
        schema_dir: &Path,
        cloud_client: &'a CloudClient,
    ) -> anyhow::Result<Handle<'a>> {
        match name {
            InstanceName::Local(name) => match InstanceInfo::try_read(name)? {
                Some(info) => Ok(Handle {
                    name: name.into(),
                    instance: InstanceKind::Portable(info),
                    project_dir: project_dir.into(),
                    schema_dir: schema_dir.into(),
                    database: None,
                }),
                None => Ok(Handle {
                    name: name.into(),
                    instance: InstanceKind::Remote,
                    project_dir: project_dir.into(),
                    schema_dir: schema_dir.into(),
                    database: None,
                }),
            },
            InstanceName::Cloud {
                org_slug,
                name: inst_name,
            } => Ok(Handle {
                name: name.to_string(),
                instance: InstanceKind::Cloud {
                    org_slug: org_slug.to_owned(),
                    name: inst_name.to_owned(),
                    cloud_client,
                },
                database: None,
                project_dir: project_dir.into(),
                schema_dir: schema_dir.into(),
            }),
        }
    }
    pub fn get_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::new();
        builder.instance(&self.name)?;
        if let Some(database) = &self.database {
            builder.database(database)?;
        }
        Ok(builder)
    }
    pub fn get_default_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::new();
        builder.instance(&self.name)?;
        Ok(builder)
    }
    pub async fn get_default_connection(&self) -> anyhow::Result<Connection> {
        Ok(Connection::connect(&self.get_default_builder()?.build_env().await?, QUERY_TAG).await?)
    }
    pub async fn get_connection(&self) -> anyhow::Result<Connection> {
        Ok(Connection::connect(&self.get_builder()?.build_env().await?, QUERY_TAG).await?)
    }
    #[tokio::main(flavor = "current_thread")]
    pub async fn get_version(&self) -> anyhow::Result<ver::Build> {
        let mut conn = self.get_default_connection().await?;
        anyhow::Ok(conn.get_version().await?.clone())
    }
    fn check_version(&self, ver_query: &Query) {
        match self.get_version() {
            Ok(inst_ver) if ver_query.matches(&inst_ver) => {}
            Ok(inst_ver) => {
                print::warn!(
                    "WARNING: existing instance has version {}, \
                    but {} is required by {CONFIG_FILE_DISPLAY_NAME}",
                    inst_ver,
                    ver_query.display()
                );
            }
            Err(e) => {
                log::warn!("Could not check instance's version: {:#}", e);
            }
        }
    }
}

#[context("cannot read schema directory `{}`", path.display())]
fn find_schema_files(path: &Path) -> anyhow::Result<bool> {
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let is_schema_file = entry
            .file_name()
            .to_str()
            .map(is_schema_file)
            .unwrap_or(false);
        if is_schema_file {
            return Ok(true);
        }
    }
    return Ok(false);
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

#[context("cannot create default schema in `{}`", dir.display())]
fn write_schema_default(dir: &Path, version: &Query) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("migrations"))?;
    let default = dir.join(format!("default.{BRANDING_SCHEMA_FILE_EXT}"));
    let tmp = tmp_file_path(&default);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, DEFAULT_SCHEMA)?;
    fs::rename(&tmp, &default)?;

    if version.is_nonrecursive_access_policies_needed() {
        let futures = dir.join(format!("futures.{BRANDING_SCHEMA_FILE_EXT}"));
        let tmp = tmp_file_path(&futures);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, FUTURES_SCHEMA)?;
        fs::rename(&tmp, &futures)?;
    };
    if version.is_simple_scoping_needed() {
        let futures = dir.join(format!("scoping.{BRANDING_SCHEMA_FILE_EXT}"));
        let tmp = tmp_file_path(&futures);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, SIMPLE_SCOPING_SCHEMA)?;
        fs::rename(&tmp, &futures)?;
    };
    Ok(())
}

#[context("cannot write config `{}`", path.display())]
fn write_config(path: &Path, version: &Query) -> anyhow::Result<()> {
    let text = config::format_config(version);
    let tmp = tmp_file_path(path);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
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

fn ask_local_version(options: &Init) -> anyhow::Result<(Query, PackageInfo)> {
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
    options: &Init,
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

#[context("cannot read instance name of {:?}", stash_dir)]
pub fn instance_name(stash_dir: &Path) -> anyhow::Result<InstanceName> {
    let inst = fs::read_to_string(stash_dir.join("instance-name"))?;
    InstanceName::from_str(inst.trim())
}

#[context("cannot read database name of {:?}", stash_dir)]
pub fn database_name(stash_dir: &Path) -> anyhow::Result<Option<String>> {
    let inst = match fs::read_to_string(stash_dir.join("database")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e)?,
    };
    Ok(Some(inst.trim().into()))
}

pub fn unlink(options: &Unlink, opts: &crate::options::Options) -> anyhow::Result<()> {
    let Some((project_dir, _)) = project_dir(options.project_dir.as_deref())? else {
        anyhow::bail!("`{CONFIG_FILE_DISPLAY_NAME}` not found, unable to unlink instance.");
    };
    let canon = fs::canonicalize(&project_dir)
        .with_context(|| format!("failed to canonicalize dir {project_dir:?}"))?;
    let stash_path = get_stash_path(&canon)?;

    if stash_path.exists() {
        if options.destroy_server_instance {
            let inst = instance_name(&stash_path)?;
            if !options.non_interactive {
                let q = question::Confirm::new_dangerous(format!(
                    "Do you really want to unlink \
                             and delete instance {inst}?"
                ));
                if !q.ask()? {
                    print::error!("Canceled.");
                    return Ok(());
                }
            }
            let inst_name = inst.to_string();
            let mut project_dirs = find_project_dirs_by_instance(&inst_name)?;
            if project_dirs.len() > 1 {
                project_dirs
                    .iter()
                    .position(|d| d == &stash_path)
                    .map(|pos| project_dirs.remove(pos));
                destroy::print_warning(&inst_name, &project_dirs);
                Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
            }
            if options.destroy_server_instance {
                destroy::force_by_name(&inst, opts)?;
            }
        } else {
            match fs::read_to_string(stash_path.join("instance-name")) {
                Ok(name) => {
                    msg!("Unlinking instance {}", name.emphasize());
                }
                Err(e) => {
                    print::error!("Cannot read instance name: {e}");
                    eprintln!("Removing project configuration directory...");
                }
            };
        }
        fs::remove_dir_all(&stash_path)?;
    } else {
        log::warn!("no project directory exists");
    }
    Ok(())
}

pub fn project_dir(cli_option: Option<&Path>) -> anyhow::Result<Option<(PathBuf, PathBuf)>> {
    // Create a temporary runtime. Not efficient, but only called at CLI startup.
    let cli_option = cli_option.map(|p| p.to_owned());
    let res = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(get_project_path(cli_option.as_deref(), true))
    })
    .join()
    .unwrap()?;
    Ok(res.map(|res| (res.parent().unwrap().to_owned(), res)))
}

pub fn info(options: &Info) -> anyhow::Result<()> {
    let Some((root, _)) = project_dir(options.project_dir.as_deref())? else {
        anyhow::bail!("`{CONFIG_FILE_DISPLAY_NAME}` not found, unable to get project info.");
    };
    let stash_dir = get_stash_path(&root)?;
    if !stash_dir.exists() {
        msg!(
            "{} {} Run `{BRANDING_CLI_CMD} project init`.",
            print::err_marker(),
            "Project is not initialized.".emphasize()
        );
        return Err(ExitCode::new(1).into());
    }
    let instance_name = fs::read_to_string(stash_dir.join("instance-name"))?;
    let cloud_profile_file = stash_dir.join("cloud-profile");
    let cloud_profile = cloud_profile_file
        .exists()
        .then(|| fs::read_to_string(cloud_profile_file))
        .transpose()?;

    let item = options
        .get
        .as_deref()
        .or(options.instance_name.then_some("instance-name"));
    if let Some(item) = item {
        match item {
            "instance-name" => {
                if options.json {
                    println!("{}", serde_json::to_string(&instance_name)?);
                } else {
                    println!("{instance_name}");
                }
            }
            "cloud-profile" => {
                if options.json {
                    println!("{}", serde_json::to_string(&cloud_profile)?);
                } else if let Some(profile) = cloud_profile {
                    println!("{profile}");
                }
            }
            _ => unreachable!(),
        }
    } else if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonInfo {
                instance_name: &instance_name,
                cloud_profile: cloud_profile.as_deref(),
                root: &root,
            })?
        );
    } else {
        let root = root.display().to_string();
        let mut rows: Vec<(&str, String)> =
            vec![("Instance name", instance_name), ("Project root", root)];
        if let Some(profile) = cloud_profile.as_deref() {
            rows.push((concatcp!(BRANDING_CLOUD, " profile"), profile.to_string()));
        }
        table::settings(rows.as_slice());
    }
    Ok(())
}

pub fn find_project_dirs_by_instance(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    find_project_stash_dirs("instance-name", |val| name == val, true)
        .map(|projects| projects.into_values().flatten().collect())
}

#[context("could not read project dir {:?}", stash_base())]
pub fn find_project_stash_dirs(
    get: &str,
    f: impl Fn(&str) -> bool,
    verbose: bool,
) -> anyhow::Result<HashMap<String, Vec<PathBuf>>> {
    let mut res = HashMap::new();
    let dir = match fs::read_dir(stash_base()?) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(res);
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let sub_dir = entry.path();
        if sub_dir
            .file_name()
            .and_then(|f| f.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(true)
        {
            // skip hidden files, most likely .DS_Store (see #689)
            continue;
        }
        let path = sub_dir.join(get);
        let value = match fs::read_to_string(&path) {
            Ok(value) => value.trim().to_string(),
            Err(e) => {
                if verbose {
                    log::warn!("Error reading {:?}: {}", path, e);
                }
                continue;
            }
        };
        if f(&value) {
            res.entry(value).or_default().push(entry.path());
        }
    }
    Ok(res)
}

pub fn print_instance_in_use_warning(name: &str, project_dirs: &[PathBuf]) {
    print::warn!(
        "Instance {:?} is used by the following project{}:",
        name,
        if project_dirs.len() > 1 { "s" } else { "" }
    );
    for dir in project_dirs {
        let dest = match read_project_path(dir) {
            Ok(path) => path,
            Err(e) => {
                print::error!("{e}");
                continue;
            }
        };
        eprintln!("  {}", dest.display());
    }
}

#[context("cannot read {:?}", project_dir)]
pub fn read_project_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(project_dir.join("project-path"))?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}

pub fn upgrade(options: &Upgrade, opts: &crate::options::Options) -> anyhow::Result<()> {
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

pub fn update_toml(
    options: &Upgrade,
    opts: &crate::options::Options,
    query: Query,
) -> anyhow::Result<()> {
    let Some((root, config_path)) = project_dir(options.project_dir.as_deref())? else {
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
        let name = instance_name(&stash_dir)?;
        let database = database_name(&stash_dir)?;
        let client = CloudClient::new(&opts.cloud_options)?;
        let mut inst = Handle::probe(&name, &root, schema_dir, &client)?;
        inst.database = database;

        let result = match inst.instance {
            InstanceKind::Remote => anyhow::bail!("remote instances cannot be upgraded"),
            InstanceKind::Portable(inst) => upgrade_local(options, &config, inst, &query, opts),
            InstanceKind::Wsl(_) => todo!(),
            InstanceKind::Cloud { org_slug, name, .. } => {
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
    for pd in find_project_dirs_by_instance(name)? {
        let real_pd = match read_project_path(&pd) {
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
            upgrade::print_project_upgrade_command(to_version, &None, pd);
        }
    }
    Ok(())
}

pub fn upgrade_instance(options: &Upgrade, opts: &crate::options::Options) -> anyhow::Result<()> {
    let Some((root, config_path)) = project_dir(options.project_dir.as_deref())? else {
        anyhow::bail!("`{CONFIG_FILE_DISPLAY_NAME}` not found, unable to upgrade {BRANDING} instance without an initialized project.");
    };
    let config = config::read(&config_path)?;
    let cfg_ver = &config.edgedb.server_version;
    let schema_dir = &config.project.schema_dir;

    let stash_dir = get_stash_path(&root)?;
    if !stash_dir.exists() {
        anyhow::bail!("No instance initialized.");
    }

    let instance_name = instance_name(&stash_dir)?;
    let database = database_name(&stash_dir)?;
    let client = CloudClient::new(&opts.cloud_options)?;
    let mut inst = Handle::probe(&instance_name, &root, schema_dir, &client)?;
    inst.database = database;
    let result = match inst.instance {
        InstanceKind::Remote => anyhow::bail!("remote instances cannot be upgraded"),
        InstanceKind::Portable(inst) => upgrade_local(options, &config, inst, cfg_ver, opts),
        InstanceKind::Wsl(_) => todo!(),
        InstanceKind::Cloud { org_slug, name, .. } => {
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
    cmd: &Upgrade,
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
                &options::Upgrade {
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
    cmd: &Upgrade,
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
