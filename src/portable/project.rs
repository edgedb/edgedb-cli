use std::env;
use std::io;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::borrow::Cow;
use std::str::FromStr;

use anyhow::Context;
use async_std::task;
use clap::{ArgSettings, ValueHint};
use fn_error_context::context;
use rand::{thread_rng, Rng};
use sha1::Digest;

use edgedb_client::client::Connection;
use edgedb_client::Builder;
use edgedb_cli_derive::EdbClap;

use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::platform::{path_bytes, bytes_to_path};
use crate::platform::{tmp_file_path, symlink_dir, config_dir};
use crate::portable::config;
use crate::portable::control;
use crate::portable::create;
use crate::portable::destroy;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{InstanceInfo, Paths, allocate_port, is_valid_name};
use crate::portable::options::{self, instance_name_opt, StartConf};
use crate::portable::platform::{optional_docker_check};
use crate::portable::repository::{self, Channel, Query, PackageInfo};
use crate::portable::upgrade;
use crate::portable::ver;
use crate::portable::windows;
use crate::print::{self, echo, Highlight};
use crate::question;
use crate::table;




const DEFAULT_ESDL: &str = "\
    module default {\n\
    \n\
    }\n\
";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectInfo {
    instance_name: String,
    stash_dir: PathBuf,
}

#[derive(EdbClap, Debug, Clone)]
pub struct ProjectCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Initialize a new or existing project
    #[edb(inherit(crate::options::CloudOptions))]
    Init(Init),
    /// Clean-up the project configuration
    #[edb(inherit(crate::options::CloudOptions))]
    Unlink(Unlink),
    /// Get various metadata about the project
    Info(Info),
    /// Upgrade EdgeDB instance used for the current project
    ///
    /// This command has two modes of operation.
    ///
    /// Upgrade instance to a version specified in `edgedb.toml`:
    ///
    ///     project upgrade
    ///
    /// Update `edgedb.toml` to a new version and upgrade the instance:
    ///
    ///     project upgrade --to-latest
    ///     project upgrade --to-version=1-beta2
    ///     project upgrade --to-nightly
    ///
    /// In all cases your data is preserved and converted using dump/restore
    /// mechanism. This might fail if lower version is specified (for example
    /// if upgrading from nightly to the stable version).
    Upgrade(Upgrade),
    /// Manipulate EdgeDB instance linked to the current project.
    Instance(Instance),
}

#[derive(EdbClap, Debug, Clone)]
pub struct Init {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Specifies the desired EdgeDB server version
    #[clap(long)]
    pub server_version: Option<Query>,

    /// Specifies whether the existing EdgeDB server instance
    /// should be linked with the project
    #[clap(long)]
    pub link: bool,

    /// Specifies the EdgeDB server instance to be associated with the project
    #[clap(long, validator(instance_name_opt))]
    pub server_instance: Option<String>,

    /// Specifies whether to start EdgeDB automatically
    #[clap(long, possible_values=&["auto", "manual"][..])]
    pub server_start_conf: Option<StartConf>,

    /// Skip running migrations
    ///
    /// There are two main use cases for this option:
    /// 1. With `--link` option to connect to a datastore with existing data
    /// 2. To initialize a new instance but then restore dump to it
    #[clap(long)]
    pub no_migrations: bool,

    /// Run in non-interactive mode (accepting all defaults)
    #[clap(long)]
    pub non_interactive: bool,

    /// Use EdgeDB Cloud to initialize this project
    #[clap(long, hide=true)]
    pub cloud: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Unlink {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// If specified, the associated EdgeDB instance is destroyed by running
    /// `edgedb instance destroy`.
    #[clap(long, short='D')]
    pub destroy_server_instance: bool,

    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Info {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Display only the instance name
    #[clap(long)]
    pub instance_name: bool,

    /// Output in JSON format
    #[clap(long)]
    pub json: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Upgrade {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Upgrade to a latest stable version
    #[clap(long)]
    pub to_latest: bool,

    /// Upgrade to a specified major version
    #[clap(long)]
    pub to_version: Option<ver::Filter>,

    /// Upgrade to a latest nightly version
    #[clap(long)]
    pub to_nightly: bool,

    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Force upgrade process even if there is no new version
    #[clap(long)]
    pub force: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Instance {
    #[clap(subcommand)]
    pub subcommand: InstanceCommand,
}

#[derive(EdbClap, Debug, Clone)]
pub enum InstanceCommand {
    /// Start EdgeDB instance linked to the current project.
    Start(Start),
}


#[derive(EdbClap, Debug, Clone)]
pub struct Start {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    #[clap(long)]
    #[cfg_attr(target_os="linux",
        clap(about="Start the server in the foreground rather than using \
                    systemd to manage the process (note: you might need to \
                    stop non-foreground instance first)"))]
    #[cfg_attr(target_os="macos",
        clap(about="Start the server in the foreground rather than using \
                    launchctl to manage the process (note: you might need to \
                    stop non-foreground instance first)"))]
    pub foreground: bool,

    /// With `--foreground` stops server running in background. And restarts
    /// the service back on exit.
    #[clap(long, conflicts_with="managed_by")]
    pub auto_restart: bool,

    #[clap(long, setting=ArgSettings::Hidden)]
    #[clap(possible_values=&["systemd", "launchctl", "edgedb-cli"][..])]
    #[clap(conflicts_with="auto_restart")]
    pub managed_by: Option<String>,
}

pub struct Handle {
    name: String,
    instance: InstanceKind,
}

pub struct WslInfo {
}

pub enum InstanceKind {
    Remote,
    Portable(InstanceInfo),
    Wsl(WslInfo),
}

#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
struct JsonInfo<'a> {
    instance_name: &'a str,
    root: &'a Path,
}


pub fn init(options: &Init, opts: &crate::options::Options) -> anyhow::Result<()> {
    if optional_docker_check()? {
        print::error(
            "`edgedb project init` in a Docker container is not supported.",
        );
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    match &options.project_dir {
        Some(dir) => {
            let dir = fs::canonicalize(&dir)?;
            if dir.join("edgedb.toml").exists() {
                if options.link {
                    link(options, &dir, opts)?
                } else {
                    init_existing(options, &dir, &opts.cloud_options)?
                }
            } else {
                if options.link {
                    anyhow::bail!(
                        "`edgedb.toml` was not found, unable to link an EdgeDB \
                        instance with uninitialized project, to initialize \
                        a new project run command without `--link` flag")
                }

                init_new(options, &dir, opts)?
            }
        }
        None => {
            let base_dir = env::current_dir()
                .context("failed to get current directory")?;
            if let Some(dir) = search_dir(&base_dir)? {
                let dir = fs::canonicalize(&dir)?;
                if options.link {
                    link(options, &dir, opts)?
                } else {
                    init_existing(options, &dir, &opts.cloud_options)?
                }
            } else {
                if options.link {
                    anyhow::bail!(
                        "`edgedb.toml` was not found, unable to link an EdgeDB \
                        instance with uninitialized project, to initialize \
                        a new project run command without `--link` flag")
                }

                let dir = fs::canonicalize(&base_dir)?;
                init_new(options, &dir, opts)?
            }
        }
    };
    Ok(())
}

fn ask_existing_instance_name() -> anyhow::Result<String> {
    let instances = credentials::all_instance_names()?;

    let mut q =
        question::String::new("Specify the name of EdgeDB instance \
                               to link with this project");
    loop {
        let target_name = q.ask()?;

        if instances.contains(&target_name) {
            return Ok(target_name);
        } else {
            print::error(format!("Instance {:?} doesn't exist", target_name));
        }
    }
}

fn link(options: &Init, project_dir: &Path, opts: &crate::options::Options)
    -> anyhow::Result<ProjectInfo>
{
    echo!("Found `edgedb.toml` in", project_dir.display());
    echo!("Linking project...");

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        anyhow::bail!("Project is already linked");
    }

    let config_path = project_dir.join("edgedb.toml");
    let config = config::read(&config_path)?;
    let ver_query = config.edgedb.server_version;

    let name = if let Some(name) = &options.server_instance {
        if options.cloud {
            let client = CloudClient::new(&opts.cloud_options)?;
            task::block_on(crate::cloud::ops::link_existing_cloud_instance(
                &client, name,
            ))?
        }
        name.clone()
    } else if options.non_interactive {
        anyhow::bail!("Existing instance name should be specified \
                       with `--server-instance` argument when linking project \
                       in non-interactive mode")
    } else if options.cloud {
        let client = CloudClient::new(&opts.cloud_options)?;
        task::block_on(crate::cloud::ops::ask_link_existing_cloud_instance(&client))?
    } else {
        ask_existing_instance_name()?
    };
    let inst = Handle::probe(&name)?;
    inst.check_version(&ver_query);
    do_link(&inst, options, project_dir, &stash_dir)
}

fn do_link(inst: &Handle, options: &Init, project_dir: &Path, stash_dir: &Path)
    -> anyhow::Result<ProjectInfo>
{
    write_stash_dir(&stash_dir, &project_dir, &inst.name)?;

    if !options.no_migrations {
        task::block_on(migrate(inst, !options.non_interactive))?;
    }

    print::success("Project linked");
    if let Some(dir) = &options.project_dir {
        eprintln!(
            "To connect to {}, navigate to {} and run `edgedb`",
            inst.name,
            dir.display()
        );
    } else {
        eprintln!("To connect to {}, run `edgedb`", inst.name);
    }

    Ok(ProjectInfo {
        instance_name: inst.name.clone(),
        stash_dir: stash_dir.into(),
    })
}

fn ask_name(dir: &Path, options: &Init) -> anyhow::Result<(String, bool)> {
    let instances = credentials::all_instance_names()?;
    let default_name = if let Some(name) = &options.server_instance {
        name.clone()
    } else {
        let path_stem = dir.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("edgedb");
        let stem = path_stem
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
        let stem = stem.trim_matches('_');
        let stem: Cow<str> = if stem.is_empty() {
            "inst".into()
        } else if stem.chars().next().expect("not empty").is_numeric() {
            format!("_{}", stem).into()
        } else {
            stem.into()
        };
        let mut name = stem.to_string();

        while instances.contains(&name) {
            name = format!("{}_{:04}",
                stem, thread_rng().gen_range(0..10000));
        }
        name
    };
    if options.non_interactive {
        if instances.contains(&default_name) {
            anyhow::bail!(format!("Instance {:?} already exists, \
                           to link project with it pass `--link` \
                           flag explicitly",
                           default_name))
        }

        return Ok((default_name, false))
    }
    let mut q = question::String::new(
        "Specify the name of EdgeDB instance to use with this project"
    );
    q.default(&default_name);
    loop {
        let target_name = q.ask()?;
        if !is_valid_name(&target_name) {
            print::error("Instance name must be a valid identifier, \
                         (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)");
            continue;
        }
        if instances.contains(&target_name) {
            let confirm = question::Confirm::new(
                format!("Do you want to use existing instance {:?} \
                         for the project?",
                         target_name)
            );
            if confirm.ask()? {
                return Ok((target_name, true));
            }
        } else {
            return Ok((target_name, false))
        }
    }
}

fn ask_link_cloud_instance(options: &Init, name: &str) -> anyhow::Result<()> {
    if options.non_interactive {
        anyhow::bail!(format!(
                    "Cloud instance {:?} already exists, \
                           to link project with it pass `--link` \
                           flag explicitly",
                    name
                ));
    } else {
        let confirm = question::Confirm::new(format!(
            "Do you want to use existing Cloud instance {:?} for the project?",
            name
        ));
        if confirm.ask()? {
            Ok(())
        } else {
            anyhow::bail!("Aborted.");
        }
    }
}

fn ask_start_conf(options: &Init) -> anyhow::Result<StartConf> {
    if let Some(conf) = &options.server_start_conf {
        return Ok(*conf);
    }
    if options.non_interactive {
        return Ok(StartConf::Auto);
    }
    let confirm = question::Confirm::new(
        "Do you want to start instance automatically on login?"
    );
    if confirm.ask()? {
        Ok(StartConf::Auto)
    } else {
        Ok(StartConf::Manual)
    }
}

pub fn init_existing(options: &Init, project_dir: &Path, cloud_options: &crate::options::CloudOptions)
    -> anyhow::Result<ProjectInfo>
{
    echo!("Found `edgedb.toml` in", project_dir.display());
    echo!("Initializing project...");

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("Project is already initialized.");
    }

    let config_path = project_dir.join("edgedb.toml");
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;
    let config = config::read(&config_path)?;

    let ver_query = if let Some(sver) = &options.server_version {
        sver.clone()
    } else {
        config.edgedb.server_version
    };
    let (name, exists) = ask_name(project_dir, options)?;

    let start_conf = if exists {
        if options.server_start_conf.is_some() {
            log::warn!("Linking to existing instance. \
                `--server-start-conf` is ignored.");
        }
        let inst = Handle::probe(&name)?;
        inst.check_version(&ver_query);
        return do_link(&inst, options, project_dir, &stash_dir);
    } else if options.cloud {
        StartConf::Auto
    } else {
        ask_start_conf(options)?
    };

    echo!("Checking EdgeDB versions...");

    if options.cloud {
        let mut client = CloudClient::new(cloud_options)?;
        if !client.is_logged_in {
            if !options.non_interactive {
                let mut q = question::Confirm::new(
                    "You're not authenticated to the EdgeDB Cloud yet, login now?",
                );
                if q.default(true).ask()? {
                    task::block_on(crate::cloud::auth::do_login(&client))?;
                    client = CloudClient::new(cloud_options)?;
                } else {
                    anyhow::bail!("Aborted.");
                }
            }
            client.ensure_authenticated(false)?;
        };
        let org = task::block_on(ask_cloud_org(&client, options))?;
        if let Some(_instance) = task::block_on(crate::cloud::ops::find_cloud_instance_by_name(
            &name, &client,
        ))? {
            ask_link_cloud_instance(options, &name)?;
            task::block_on(crate::cloud::ops::link_existing_cloud_instance(&client, &name))?;
            let inst = Handle::probe(&name)?;
            inst.check_version(&ver_query);
            return do_link(&inst, options, project_dir, &stash_dir);
        }
        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                      if schema_files { "(non-empty)" } else { "(empty)" }),
             &schema_dir.display().to_string()),
            // ("Version", &format!("{:?}", version)),
            ("Instance name", &name),
        ]);
        if !schema_files {
            write_schema_default(&schema_dir)?;
        }

        do_cloud_init(name, org, &stash_dir, &project_dir, options, &client)  // , version)
    } else {
        let pkg = repository::get_server_package(&ver_query)?
            .with_context(||
                format!("cannot find package matching {}", ver_query.display()))?;

        let meth = if cfg!(windows) {
            "WSL"
        } else {
            "portable package"
        };
        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                      if schema_files { "(non-empty)" } else { "(empty)" }),
             &schema_dir.display().to_string()),
            ("Installation method", meth),
            ("Start configuration", start_conf.as_str()),
            ("Version", &pkg.version.to_string()),
            ("Instance name", &name),
        ]);

        if !schema_files {
            write_schema_default(&schema_dir)?;
        }

        do_init(&name, &pkg, &stash_dir, &project_dir, start_conf, options)
    }
}

fn do_init(name: &str, pkg: &PackageInfo,
           stash_dir: &Path, project_dir: &Path, start_conf: StartConf,
           options: &Init)
    -> anyhow::Result<ProjectInfo>
{
    let port = allocate_port(name)?;
    let paths = Paths::get(&name)?;

    let (instance, svc_result) = if cfg!(windows) {
        let q = repository::Query::from_version(&pkg.version.specific())?;
        windows::create_instance(&options::Create {
            name: name.into(),
            nightly: q.is_nightly(),
            version: q.version,
            port: Some(port),
            start_conf,
            default_database: "edgedb".into(),
            default_user: "edgedb".into(),
            cloud: false,
            cloud_org: None,
        }, port, &paths)?;
        let svc_result = create::create_service(&InstanceInfo {
            name: name.into(),
            installation: None,
            port,
            start_conf,
        });
        (InstanceKind::Wsl(WslInfo {}), svc_result)
    } else {
        let inst = install::package(&pkg).context("error installing EdgeDB")?;
        let info = InstanceInfo {
            name: name.into(),
            installation: Some(inst),
            port,
            start_conf,
        };
        create::bootstrap(&paths, &info, "edgedb", "edgedb")?;
        let svc_result = create::create_service(&info);
        (InstanceKind::Portable(info), svc_result)
    };

    write_stash_dir(stash_dir, project_dir, &name)?;

    let handle = Handle {
        name: name.into(),
        instance,
    };
    match (svc_result, start_conf) {
        (Ok(()), StartConf::Manual) => {
            if !options.no_migrations {
                run_and_migrate(&handle)?;
            }
            print_initialized(&name, &options.project_dir, start_conf);
        }
        (Ok(()), StartConf::Auto) => {
            if !options.no_migrations {
                task::block_on(migrate(&handle, false))?;
            }
            print_initialized(&name, &options.project_dir, start_conf);
        }
        (Err(e), _) => {
            if !options.no_migrations {
                run_and_migrate(&handle)?;
            }
            echo!("Bootstrapping complete, \
                but there was an error creating the service:",
                format_args!("{:#}", e));
            echo!("You can start it manually via:");
            echo!("  edgedb project instance start");
            return Err(ExitCode::new(exit_codes::CANNOT_CREATE_SERVICE))?;
        }
    }
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
    options: &Init,
    client: &CloudClient,
    // version: String,
) -> anyhow::Result<ProjectInfo> {
    let instance = crate::cloud::ops::CloudInstanceCreate {
        name: name.clone(),
        org,
        // version: Some(version),
        // default_database: None,
        // default_user: None,
    };
    task::block_on(
        crate::cloud::ops::create_cloud_instance(client, &instance)
    )?;
    write_stash_dir(stash_dir, project_dir, &name)?;
    if !options.no_migrations {
        let handle = Handle {
            name: name.clone(),
            instance: InstanceKind::Remote,
        };
        task::block_on(migrate(&handle, false))?;
    }
    print_initialized(&name, &options.project_dir, StartConf::Auto);
    Ok(ProjectInfo {
        instance_name: name,
        stash_dir: stash_dir.into(),
    })
}

pub fn init_new(options: &Init, project_dir: &Path, opts: &crate::options::Options)
    -> anyhow::Result<ProjectInfo>
{
    eprintln!("No `edgedb.toml` found in `{}` or above",
              project_dir.display());

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        anyhow::bail!("Project was already initialized \
                       but then `edgedb.toml` was deleted. \
                       Please run `edgedb project unlink -D` to \
                       cleanup old database instance.");
    }

    if options.non_interactive {
        eprintln!("Initializing new project...");
    } else {
        let mut q = question::Confirm::new(
            "Do you want to initialize a new project?"
        );
        q.default(true);
        if !q.ask()? {
            return Err(ExitCode::new(0).into());
        }
    }

    let config_path = project_dir.join("edgedb.toml");
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;

    let (name, exists) = ask_name(project_dir, options)?;

    if exists {
        if options.server_start_conf.is_some() {
            log::warn!("Linking to existing instance. \
                `--server-start-conf` is ignored.");
        }
        let inst = Handle::probe(&name)?;
        write_config(&config_path,
                     &Query::from_version(&inst.get_version()?.specific())?)?;
        if !schema_files {
            write_schema_default(&schema_dir)?;
        }
        return do_link(&inst, options, project_dir, &stash_dir);
    };

    echo!("Checking EdgeDB versions...");

    if options.cloud {
        let mut client = CloudClient::new(&opts.cloud_options)?;
        if !client.is_logged_in {
            if !options.non_interactive {
                let mut q = question::Confirm::new(
                    "You're not authenticated to the EdgeDB Cloud yet, login now?",
                );
                if q.default(true).ask()? {
                    task::block_on(crate::cloud::auth::do_login(&client))?;
                    client = CloudClient::new(&opts.cloud_options)?;
                } else {
                    anyhow::bail!("Aborted.");
                }
            }
            client.ensure_authenticated(false)?;
        };
        let org = task::block_on(ask_cloud_org(&client, options))?;
        if let Some(_instance) = task::block_on(crate::cloud::ops::find_cloud_instance_by_name(
            &name, &client,
        ))? {
            ask_link_cloud_instance(options, &name)?;
            task::block_on(crate::cloud::ops::link_existing_cloud_instance(&client, &name))?;
            let inst = Handle::probe(&name)?;
            write_config(&config_path,
                         &Query::from_version(&inst.get_version()?.specific())?)?;
            if !schema_files {
                write_schema_default(&schema_dir)?;
            }
            return do_link(&inst, options, project_dir, &stash_dir);
        }
        let version = ask_cloud_version(options)?;
        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                      if schema_files { "(non-empty)" } else { "(empty)" }),
             &schema_dir.display().to_string()),
            ("Version", &format!("{:?}", version)),
            ("Instance name", &name),
        ]);
        let ver_query = Query::from_str(&version)?;
        write_config(&config_path, &ver_query)?;
        if !schema_files {
            write_schema_default(&schema_dir)?;
        }

        do_cloud_init(name, org, &stash_dir, &project_dir, options, &client)  // , version)
    } else {
        let pkg = ask_version(options)?;
        let start_conf = ask_start_conf(options)?;

        let meth = if cfg!(windows) {
            "WSL"
        } else {
            "portable package"
        };
        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                      if schema_files { "(non-empty)" } else { "(empty)" }),
             &schema_dir.display().to_string()),
            ("Installation method", meth),
            ("Start configuration", start_conf.as_str()),
            ("Version", &pkg.version.to_string()),
            ("Instance name", &name),
        ]);

        let ver_query = Query::from_version(&pkg.version.specific())?;
        write_config(&config_path, &ver_query)?;
        if !schema_files {
            write_schema_default(&schema_dir)?;
        }

        do_init(&name, &pkg, &stash_dir, &project_dir, start_conf, options)
    }
}

pub fn search_dir(base: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut path = base;
    if path.join("edgedb.toml").exists() {
        return Ok(Some(path.into()));
    }
    while let Some(parent) = path.parent() {
        if parent.join("edgedb.toml").exists() {
            return Ok(Some(parent.into()));
        }
        path = parent;
    }
    Ok(None)
}

fn hash(path: &Path) -> anyhow::Result<String> {
    Ok(hex::encode(sha1::Sha1::new_with_prefix(path_bytes(path)?).finalize()))
}

fn stash_name(path: &Path) -> anyhow::Result<OsString> {
    let hash = hash(path)?;
    let base = path.file_name().ok_or_else(|| anyhow::anyhow!("bad path"))?;
    let mut base = base.to_os_string();
    base.push("-");
    base.push(&hash);
    return Ok(base);
}

pub fn stash_base() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

pub fn stash_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let hname = stash_name(project_dir)?;
    Ok(stash_base()?.join(hname))
}

fn run_and_migrate(info: &Handle) -> anyhow::Result<()> {
    match &info.instance {
        InstanceKind::Portable(inst) => {
            control::ensure_runstate_dir(&info.name)?;
            let mut cmd = control::get_server_cmd(inst)?;
            cmd.background_for(migrate(info, false))?;
            Ok(())
        }
        InstanceKind::Wsl(_) => {
            let mut cmd = windows::server_cmd(&info.name)?;
            cmd.background_for(migrate(info, false))?;
            Ok(())
        }
        InstanceKind::Remote => {
            anyhow::bail!("remote instance is not running, \
                          cannot run migrations");
        }
    }
}

fn start(handle: &Handle) -> anyhow::Result<()> {
    match &handle.instance {
        InstanceKind::Portable(inst) => {
            control::do_start(&inst)?;
            Ok(())
        }
        InstanceKind::Wsl(_) => {
            windows::daemon_start(&handle.name)?;
            Ok(())
        }
        InstanceKind::Remote => {
            anyhow::bail!("remote instance is not running, \
                          cannot run migrations");
        }
    }
}

async fn migrate(inst: &Handle, ask_for_running: bool)
    -> anyhow::Result<()>
{
    use crate::commands::Options;
    use crate::commands::parser::{Migrate, MigrationConfig};
    use Action::*;

    #[derive(Clone, Copy)]
    enum Action {
        Retry,
        Service,
        Run,
        Skip,
    }

    echo!("Applying migrations...");

    let mut conn = loop {
        match inst.get_connection().await {
            Ok(conn) => break conn,
            Err(e) if ask_for_running && inst.instance.is_local() => {
                print::error(e);
                let mut q = question::Numeric::new(
                    format!(
                        "Cannot connect to an instance {:?}. What to do?",
                        inst.name,
                    )
                );
                q.option("Start the service (if possible).",
                    Service);
                q.option("Start in the foreground, \
                          apply migrations and shutdown.",
                    Run);
                q.option("I have just started it manually. Try again!",
                    Retry);
                q.option("Skip migrations.",
                    Skip);
                match q.ask()? {
                    Service => match start(inst) {
                        Ok(()) => continue,
                        Err(e) => {
                            print::error(e);
                            continue;
                        }
                    }
                    Run => {
                        run_and_migrate(inst)?;
                        return Ok(());
                    }
                    Retry => continue,
                    Skip => {
                        print::warn("Skipping migrations.");
                        echo!("Once service is running, \
                            you can apply migrations by running:\n  \
                              edgedb migrate");
                        return Ok(());
                    }
                }
            }
            Err(e) => return Err(e)?,
        };
    };

    migrations::migrate(
        &mut conn,
        &Options {
            command_line: true,
            styler: None,
            conn_params: Connector::new(Ok(inst.get_builder().await?)),
        },
        &Migrate {
            cfg: MigrationConfig {
                schema_dir: "./dbschema".into(),
            },
            quiet: false,
            to_revision: None,
        }).await?;
    Ok(())
}

#[context("error writing project dir {:?}", dir)]
fn write_stash_dir(dir: &Path, project_dir: &Path, instance_name: &str)
    -> anyhow::Result<()>
{
    let tmp = tmp_file_path(&dir);
    fs::create_dir_all(&tmp)?;
    fs::write(&tmp.join("project-path"), path_bytes(project_dir)?)?;
    fs::write(&tmp.join("instance-name"), instance_name.as_bytes())?;

    let lnk = tmp.join("project-link");
    symlink_dir(project_dir, &lnk)
        .map_err(|e| {
            log::info!("Error symlinking project at {:?}: {}", lnk, e);
        }).ok();
    fs::rename(&tmp, dir)?;
    Ok(())
}

impl InstanceKind {
    fn is_local(&self) -> bool {
        match self {
            InstanceKind::Wsl(_) => true,
            InstanceKind::Portable(_) => true,
            InstanceKind::Remote => false,
        }
    }
}

impl Handle {
    pub fn probe(name: &str) -> anyhow::Result<Handle> {
        if let Some(info) = InstanceInfo::try_read(name)? {
            return Ok(Handle {
                name: name.into(),
                instance: InstanceKind::Portable(info),
            });
        };
        Ok(Handle {
            name: name.into(),
            instance: InstanceKind::Remote,
        })
    }
    pub async fn get_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::uninitialized();
        builder.read_instance(&self.name).await?;
        Ok(builder)
    }
    pub async fn get_connection(&self) -> anyhow::Result<Connection> {
        Ok(self.get_builder().await?.build()?.connect().await?)
    }
    pub fn get_version(&self) -> anyhow::Result<ver::Build> {
        task::block_on(async {
            let mut conn = self.get_connection().await?;
            let ver = conn.query_row::<String, _>(r###"
                SELECT sys::get_version_as_str()
            "###, &()).await?;
            Ok(ver.parse()?)
        })
    }
    fn check_version(&self, ver_query: &Query) {
        match self.get_version() {
            Ok(inst_ver) if ver_query.matches(&inst_ver) => {}
            Ok(inst_ver) => {
                print::warn(format!(
                    "WARNING: existing instance has version {}, \
                    but {} is required by `edgedb.toml`",
                    inst_ver, ver_query.display(),
                ));
            }
            Err(e) => {
                log::warn!("Could not check instance's version: {:#}", e);
            }
        }
    }
}

#[context("cannot read schema directory `{}`", path.display())]
fn find_schema_files(path: &Path) -> anyhow::Result<bool> {
    let dir = match fs::read_dir(&path) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let is_esdl = entry.file_name().to_str()
            .map(|x| x.ends_with(".esdl"))
            .unwrap_or(false);
        if is_esdl {
            return Ok(true);
        }
    }
    return Ok(false);
}

fn print_initialized(name: &str, dir_option: &Option<PathBuf>,
    start_conf: StartConf)
{
    print::success("Project initialized.");
    if start_conf == StartConf::Manual {
        echo!("To start the server run:");
        echo!("  edgedb instance start".command_hint(),
              name.escape_default().command_hint());
    } else {
        if let Some(dir) = dir_option {
            echo!("To connect to", name.emphasize();
                  ", navigate to", dir.display(), "and run `edgedb`");
        } else {
            echo!("To connect to", name.emphasize(); ", run `edgedb`");
        }
    }
}

#[context("cannot create default schema in `{}`", dir.display())]
fn write_schema_default(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(&dir.join("migrations"))?;
    let default = dir.join("default.esdl");
    let tmp = tmp_file_path(&default);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, DEFAULT_ESDL)?;
    fs::rename(&tmp, &default)?;
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

fn ask_version(options: &Init) -> anyhow::Result<PackageInfo> {
    let ver_query = options.server_version.clone().unwrap_or(Query::stable());
    if options.non_interactive || options.server_version.is_some() {
        let pkg = repository::get_server_package(&ver_query)?
            .with_context(|| format!("no package matching {} found",
                                     ver_query.display()))?;
        return Ok(pkg);
    }
    let default = repository::get_server_package(&ver_query)?;
    let default_ver = if let Some(pkg) = &default {
        Query::from_version(&pkg.version.specific())?.as_config_value()
    } else {
        String::new()
    };
    let mut q = question::String::new(
        "Specify the version of EdgeDB to use with this project"
    );
    q.default(&default_ver);
    loop {
        let value = q.ask()?;
        let value = value.trim();
        if value == "nightly" {
            match repository::get_server_package(&Query::nightly()) {
                Ok(Some(pkg)) => return Ok(pkg),
                Ok(None) => {
                    print::error("No nightly versions found");
                    continue;
                }
                Err(e) => {
                    print::error(format!(
                        "Cannot find nightly version: {}", e
                    ));
                    continue;
                }
            }
        } else {
            let pkg = value.parse()
                .and_then(|f| Query::from_filter(&f))
                .and_then(|v| repository::get_server_package(&v));
            match pkg {
                Ok(Some(pkg)) => return Ok(pkg),
                Ok(None) => {
                    print::error("No matching packages found");
                    print_versions("Available versions")?;
                    continue;
                }
                Err(e) => {
                    print::error(e);
                    print_versions("Available versions")?;
                    continue;
                }
            }
        }
    }
}

fn ask_cloud_version(options: &Init) -> anyhow::Result<String> {
    if options.non_interactive {
        Ok("*".into())
    } else {
        let mut q = question::String::new(
            "Specify the version of EdgeDB to use with this project"
        );
        let default_version;
        if let Some(version) = &options.server_version {
            default_version = version.display().to_string();
            q.default(&default_version);
        }
        Ok(q.ask()?)
    }
}

async fn ask_cloud_org(client: &CloudClient, options: &Init) -> anyhow::Result<String> {
    let orgs: Vec<crate::cloud::ops::Org> = client.get("orgs/").await?;
    if options.non_interactive {
        Ok(orgs.into_iter().next().unwrap().id)
    } else {
        let mut q = question::Numeric::new("Choose an organization:");
        for org in orgs {
            q.option(org.name, org.id);
        }
        Ok(q.ask()?)
    }
}

fn print_versions(title: &str) -> anyhow::Result<()> {
    let mut avail = repository::get_server_packages(Channel::Stable)?;
    avail.sort_by(|a, b| b.version.cmp(&a.version));
    println!("{}: {}{}",
        title,
        avail.iter()
            .filter_map(|p| Query::from_version(&p.version.specific()).ok())
            .take(5)
            .map(|v| v.as_config_value())
            .collect::<Vec<_>>()
            .join(", "),
        if avail.len() > 5 { " ..." } else { "" },
    );
    Ok(())
}

fn search_for_unlink(base: &Path) -> anyhow::Result<PathBuf> {
    let mut path = base;
    while let Some(parent) = path.parent() {
        let canon = fs::canonicalize(&path)
            .with_context(|| {
                format!("failed to canonicalize dir {:?}", parent)
            })?;
        let stash_dir = stash_path(&canon)?;
        if stash_dir.exists() || path.join("edgedb.toml").exists() {
            return Ok(stash_dir)
        }
        path = parent;
    }
    anyhow::bail!("no project directory found");
}

#[context("cannot read instance name of {:?}", stash_dir)]
fn instance_name(stash_dir: &Path) -> anyhow::Result<String> {
    let inst = fs::read_to_string(&stash_dir.join("instance-name"))?;
    Ok(inst.trim().into())
}

pub fn unlink(options: &Unlink, opts: &crate::options::Options) -> anyhow::Result<()> {
    let stash_path = if let Some(dir) = &options.project_dir {
        let canon = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        stash_path(&canon)?
    } else {
        let base = env::current_dir()
            .context("failed to get current directory")?;
        search_for_unlink(&base)?
    };

    if stash_path.exists() {
        if options.destroy_server_instance {
            let inst = instance_name(&stash_path)?;
            if !options.non_interactive {
                let q = question::Confirm::new_dangerous(
                    format!("Do you really want to unlink \
                             and delete instance {:?}?", inst.trim())
                );
                if !q.ask()? {
                    print::error("Canceled.");
                    return Ok(())
                }
            }
            let mut project_dirs = find_project_dirs(&inst)?;
            if project_dirs.len() > 1 {
                project_dirs.iter().position(|d| d == &stash_path)
                    .map(|pos| project_dirs.remove(pos));
                destroy::print_warning(&inst, &project_dirs);
                return Err(ExitCode::new(exit_codes::NEEDS_FORCE))?;
            }
            if options.destroy_server_instance {
                destroy::force_by_name(&inst, opts)?;
            }
        } else {
            match fs::read_to_string(&stash_path.join("instance-name")) {
                Ok(name) => {
                    echo!("Unlinking instance", name.emphasize());
                }
                Err(e) => {
                    print::error(format!("Cannot read instance name: {}", e));
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


pub fn project_dir(cli_option: Option<&Path>) -> anyhow::Result<PathBuf> {
    project_dir_opt(cli_option)?
    .ok_or_else(|| {
        anyhow::anyhow!("no `edgedb.toml` found")
    })
}

pub fn project_dir_opt(cli_options: Option<&Path>)
    -> anyhow::Result<Option<PathBuf>>
{
    match cli_options {
        Some(dir) => {
            if dir.join("edgedb.toml").exists() {
                let canon = fs::canonicalize(&dir)
                    .with_context(|| {
                        format!("failed to canonicalize dir {:?}", dir)
                    })?;
                Ok(Some(canon))
            } else {
                anyhow::bail!("no `edgedb.toml` found in {:?}", dir);
            }
        }
        None => {
            let dir = env::current_dir()
                .context("failed to get current directory")?;
            if let Some(ancestor) = search_dir(&dir)? {
                let canon = fs::canonicalize(&ancestor)
                    .with_context(|| {
                        format!("failed to canonicalize dir {:?}", ancestor)
                    })?;
                Ok(Some(canon))
            } else {
                Ok(None)
            }
        }
    }
}

pub fn info(options: &Info) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        echo!(print::err_marker(),
            "Project is not initialized.".emphasize(),
            "Run `edgedb project init`.");
        return Err(ExitCode::new(1).into());
    }
    let instance_name = fs::read_to_string(stash_dir.join("instance-name"))?;

    if options.instance_name {
        if options.json {
            println!("{}", serde_json::to_string(&instance_name)?);
        } else {
            println!("{}", instance_name);
        }
    } else if options.json {
        println!("{}", serde_json::to_string_pretty(&JsonInfo {
            instance_name: &instance_name,
            root: &root,
        })?);
    } else {
        table::settings(&[
            ("Instance name", &instance_name),
            ("Project root", &root.display().to_string()),
        ]);
    }
    Ok(())
}

#[context("could not read project dir {:?}", stash_base())]
pub fn find_project_dirs(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut res = Vec::new();
    let dir = match fs::read_dir(stash_base()?) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let sub_dir = entry.path();
        if sub_dir.file_name()
            .and_then(|f| f.to_str())
            .map(|n| n.starts_with("."))
            .unwrap_or(true)
        {
            // skip hidden files, most likely .DS_Store (see #689)
            continue;
        }
        let path = sub_dir.join("instance-name");
        let inst = match fs::read_to_string(&path) {
            Ok(inst) => inst,
            Err(e) => {
                log::warn!("Error reading {:?}: {}", path, e);
                continue;
            }
        };
        if name == inst.trim() {
            res.push(entry.path());
        }
    }
    Ok(res)
}

pub fn print_instance_in_use_warning(name: &str, project_dirs: &[PathBuf]) {
    print::warn(format!(
        "Instance {:?} is used by the following project{}:",
        name,
        if project_dirs.len() > 1 { "s" } else { "" },
    ));
    for dir in project_dirs {
        let dest = match read_project_real_path(dir) {
            Ok(path) => path,
            Err(e) => {
                print::error(e);
                continue;
            }
        };
        eprintln!("  {}", dest.display());
    }
}

#[context("cannot read {:?}", project_dir)]
pub fn read_project_real_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(&project_dir.join("project-path"))?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    if options.to_version.is_some() || options.to_nightly || options.to_latest
    {
        update_toml(&options)
    } else {
        upgrade_instance(&options)
    }
}

pub fn update_toml(options: &Upgrade) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let config_path = root.join("edgedb.toml");

    // This assumes to_version.is_some() || to_nightly || to_latest
    let query = Query::from_options(options.to_nightly, &options.to_version)?;
    let pkg = repository::get_server_package(&query)?.with_context(||
        format!("cannot find package matching {}", query.display()))?;
    let pkg_ver = pkg.version.specific();

    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        log::warn!("No associated instance found.");

        if config::modify(&config_path, &query)? {
            print::success("Config updated successfully.");
        } else {
            print::success("Config is up to date.");
        }
        echo!("Run", "edgedb project init".command_hint(),
              "to initialize an instance.");
    } else {
        let name = instance_name(&stash_dir)?;
        let inst = Handle::probe(&name)?;
        let inst = match inst.instance {
            InstanceKind::Remote
                => anyhow::bail!("remote instances cannot be upgraded"),
            InstanceKind::Portable(inst) => inst,
            InstanceKind::Wsl(_) => todo!(),
        };
        let inst_ver = inst.get_version()?.specific();

        if pkg_ver > inst_ver || options.force {
            if cfg!(windows) {
                windows::upgrade(&options::Upgrade {
                    to_latest: false,
                    to_version: query.version.clone(),
                    to_nightly: query.is_nightly(),
                    name: name.clone(),
                    verbose: false,
                    force: options.force,
                    force_dump_restore: options.force,
                })?;
            } else {
                // When force is used we might upgrade to the same version, but
                // since some selector like `--to-latest` was specified we
                // assume user want to treat this upgrade as incompatible and
                // do the upgrade.  This is mostly for testing.
                if pkg_ver.is_compatible(&inst_ver) && !options.force {
                    upgrade::upgrade_compatible(inst, pkg)?;
                } else {
                    upgrade::upgrade_incompatible(inst, pkg)?;
                }
            }
            let config_version = if query.is_nightly() {
                query.clone()
            } else {
                // on `--to-latest` which is equivalent to `server-version="*"`
                // we put specific version instead
                Query::from_version(&pkg_ver)?
            };

            if config::modify(&config_path, &config_version)? {
                echo!("Remember to commit it to version control.");
            }
            print_other_project_warning(&name, &root, &query)?;
        } else {
            echo!("Latest version found", pkg.version.to_string() + ",",
                  "current instance version is",
                  inst.get_version()?.emphasize().to_string() + ".",
                  "Already up to date.");
        }
    };
    Ok(())
}

fn print_other_project_warning(name: &str, project_path: &Path,
                               to_version: &Query)
    -> anyhow::Result<()>
{
    let mut project_dirs = Vec::new();
    for pd in find_project_dirs(name)? {
        let real_pd = match read_project_real_path(&pd) {
            Ok(path) => path,
            Err(e) => {
                print::error(e);
                continue;
            }
        };
        if real_pd != project_path {
            project_dirs.push(real_pd);
        }
    }
    if !project_dirs.is_empty() {
        print::warn(format!(
            "Warning: the instance {} is still used by the following \
            projects:", name
        ));
        for pd in &project_dirs {
            eprintln!("  {}", pd.display());
        }
        eprintln!("Run the following commands to update them:");
        for pd in &project_dirs {
            upgrade::print_project_upgrade_command(&to_version, &None, pd);
        }
    }
    Ok(())
}

pub fn upgrade_instance(options: &Upgrade) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let config_path = root.join("edgedb.toml");
    let config = config::read(&config_path)?;
    let cfg_ver = config.edgedb.server_version;

    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        anyhow::bail!("No instance initialized.");
    }

    let instance_name = instance_name(&stash_dir)?;
    let inst = Handle::probe(&instance_name)?;
    let inst = match inst.instance {
        InstanceKind::Remote
            => anyhow::bail!("remote instances cannot be upgraded"),
        InstanceKind::Portable(inst) => inst,
        InstanceKind::Wsl(_) => todo!(),
    };
    let inst_ver = inst.get_version()?.specific();

    let pkg = repository::get_server_package(&cfg_ver)?.with_context(||
        format!("cannot find package matching {}", cfg_ver.display()))?;
    let pkg_ver = pkg.version.specific();

    if pkg_ver > inst_ver || options.force {
        if cfg!(windows) {
            windows::upgrade(&options::Upgrade {
                to_latest: false,
                to_version: cfg_ver.version.clone(),
                to_nightly: cfg_ver.is_nightly(),
                name: instance_name.into(),
                verbose: false,
                force: options.force,
                force_dump_restore: options.force,
            })?;
        } else {
            // When force is used we might upgrade to the same version, but
            // since some selector like `--to-latest` was specified we assume
            // user want to treat this upgrade as incompatible and do the
            // upgrade. This is mostly for testing.
            if pkg_ver.is_compatible(&inst_ver) {
                upgrade::upgrade_compatible(inst, pkg)?;
            } else {
                upgrade::upgrade_incompatible(inst, pkg)?;
            }
        }
    } else {
        echo!("EdgeDB instance is up to date with \
               the specification in the `edgedb.toml`.");
        if cfg_ver.channel != Channel::Nightly {
            if let Some(pkg) =repository::get_server_package(&Query::stable())?
            {
                echo!("New major version is available:",
                      pkg.version.emphasize());
                echo!("To update `edgedb.toml` and upgrade to this version,
                       run:\n    edgedb project upgrade --to-latest");
            }
        }
    }
    Ok(())
}

pub fn instance(cmd: &Instance) -> anyhow::Result<()> {
    use InstanceCommand::*;

    match &cmd.subcommand {
        Start(c) => start_instance(c),
    }
}

fn start_instance(options: &Start) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        echo!(print::err_marker(),
            "Project is not initialized.".emphasize(),
            "Run", "edgedb project init".command_hint(),
            "to initialize an instance.");
        return Err(ExitCode::new(1).into());
    }
    let instance_name = instance_name(&stash_dir)?;

    let start_options = options::Start {
        name: instance_name,
        foreground: options.foreground,
        auto_restart: options.auto_restart,
        managed_by: options.managed_by.clone()
    };

    if cfg!(windows) {
        windows::start(&start_options)
    } else {
        control::start(&start_options)
    }
}
