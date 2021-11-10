use std::env;
use std::io;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::borrow::Cow;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use rand::{thread_rng, Rng};

use edgedb_client::client::Connection;
use edgedb_client::Builder;

use crate::commands::ExitCode;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::platform::{tmp_file_path, path_bytes, symlink_dir, config_dir};
use crate::portable::config;
use crate::portable::create::{self, InstanceInfo};
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local;
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{self, Channel, Query, PackageInfo};
use crate::portable::ver;
use crate::print;
use crate::project::options::Init;
use crate::question;
use crate::server::create::allocate_port;
use crate::server::is_valid_name;
use crate::server::options::StartConf;
use crate::table;


const DEFAULT_ESDL: &str = "\
    module default {\n\
    \n\
    }\n\
";


pub struct Handle {
    name: String,
    instance: InstanceKind,
}

pub enum InstanceKind {
    Remote,
    Portable(InstanceInfo),
    Deprecated,
}


pub fn init(options: &Init) -> anyhow::Result<()> {
    if options.server_install_method.is_some() {
        return crate::project::init::init(options);
    }
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
                    link(options, &dir)?;
                } else {
                    init_existing(options, &dir)?;
                }
            } else {
                if options.link {
                    anyhow::bail!(
                        "`edgedb.toml` was not found, unable to link an EdgeDB \
                        instance with uninitialized project, to initialize \
                        a new project run command without `--link` flag")
                }

                init_new(options, &dir)?;
            }
        }
        None => {
            let base_dir = env::current_dir()
                .context("failed to get current directory")?;
            if let Some(dir) = search_dir(&base_dir)? {
                let dir = fs::canonicalize(&dir)?;
                if options.link {
                    link(options, &dir)?;
                } else {
                    init_existing(options, &dir)?;
                }
            } else {
                if options.link {
                    anyhow::bail!(
                        "`edgedb.toml` was not found, unable to link an EdgeDB \
                        instance with uninitialized project, to initialize
                        a new project run command without `--link` flag")
                }

                let dir = fs::canonicalize(&base_dir)?;
                init_new(options, &dir)?;
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

fn link(options: &Init, project_dir: &Path) -> anyhow::Result<()> {
    println!("Found `edgedb.toml` in `{}`", project_dir.display());
    println!("Linking project...");

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        anyhow::bail!("Project is already linked");
    }

    let config_path = project_dir.join("edgedb.toml");
    let config = config::read(&config_path)?;
    let ver_query = config.edgedb.server_version;

    let name = if let Some(name) = &options.server_instance {
        name.clone()
    } else if options.non_interactive {
        anyhow::bail!("Existing instance name should be specified \
                       with `--server-instance` argument when linking project \
                       in non-interactive mode")
    } else {
        ask_existing_instance_name()?
    };
    let inst = Handle::probe(&name)?;
    inst.check_version(&ver_query);
    do_link(&inst, options, project_dir, &stash_dir)
}

fn do_link(inst: &Handle, options: &Init, project_dir: &Path, stash_dir: &Path)
    -> anyhow::Result<()>
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

    Ok(())
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

pub fn init_existing(options: &Init, project_dir: &Path)
    -> anyhow::Result<()>
{
    println!("Found `edgedb.toml` in `{}`", project_dir.display());
    println!("Initializing project...");

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("Project is already initialized.");
    }

    let config_path = project_dir.join("edgedb.toml");
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;
    let config = config::read(&config_path)?;

    let ver_query = if options.server_version.is_some() {
        Query::from_option(&options.server_version)?
    } else {
        config.edgedb.server_version
    };
    let (name, exists) = ask_name(project_dir, options)?;

    if exists {
        if options.server_start_conf.is_some() {
            log::warn!("Linking to existing instance. \
                `--server-start-conf` is ignored.");
        }
        let inst = Handle::probe(&name)?;
        inst.check_version(&ver_query);
        return do_link(&inst, options, project_dir, &stash_dir);
    }

    println!("Checking EdgeDB versions...");

    let pkg = repository::get_server_package(&ver_query)?.with_context(||
        format!("cannot find package matching {}", ver_query.display()))?;

    table::settings(&[
        ("Project directory", &project_dir.display().to_string()),
        ("Project config", &config_path.display().to_string()),
        (&format!("Schema dir {}",
            if schema_files { "(non-empty)" } else { "(empty)" }),
            &schema_dir.display().to_string()),
        ("Installation method", "portable package"),
        ("Version", &pkg.version.to_string()),
        ("Instance name", &name),
    ]);

    if !schema_files {
        write_schema_default(&schema_dir)?;
    }

    do_init(&name, &pkg, &stash_dir, &project_dir, options)
}

fn do_init(name: &str, pkg: &PackageInfo,
           stash_dir: &Path, project_dir: &Path, options: &Init)
    -> anyhow::Result<()>
{
    let inst = install::package(&pkg).context("error installing EdgeDB")?;
    let start_conf = options.server_start_conf.unwrap_or(StartConf::Auto);
    let port = allocate_port(name)?;
    let info = InstanceInfo {
        installation: inst,
        port,
        start_conf,
    };
    let paths = local::Paths::get(&name)?;
    create::bootstrap(&paths, &info, "edgedb", "edgedb")?;

    let svc_result = create::create_service(&name, &info, &paths);

    write_stash_dir(stash_dir, project_dir, &name)?;

    let handle = Handle {
        name: name.into(),
        instance: InstanceKind::Portable(info),
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
            eprintln!("Bootstrapping complete, \
                but there was an error creating the service: {:#}", e);
            eprintln!("You can start it manually via: \n  \
                edgedb instance start --foreground {}",
                name);
            return Err(ExitCode::new(2))?;
        }
    }
    Ok(())
}

pub fn init_new(options: &Init, project_dir: &Path) -> anyhow::Result<()> {
    eprintln!("No `edgedb.toml` found in `{}` or above",
              project_dir.display());

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("Project is already initialized.");
    }

    if options.non_interactive {
        eprintln!("Initializing new project...");
    } else {
        let mut q = question::Confirm::new(
            "Do you want to initialize a new project?"
        );
        q.default(true);
        if !q.ask()? {
            return Ok(());
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
    }

    println!("Checking EdgeDB versions...");

    let pkg = ask_version(options)?;

    table::settings(&[
        ("Project directory", &project_dir.display().to_string()),
        ("Project config", &config_path.display().to_string()),
        (&format!("Schema dir {}",
            if schema_files { "(non-empty)" } else { "(empty)" }),
            &schema_dir.display().to_string()),
        ("Installation method", "portable package"),
        ("Version", &pkg.version.to_string()),
        ("Instance name", &name),
    ]);

    let ver_query = Query::from_version(&pkg.version.specific())?;
    write_config(&config_path, &ver_query)?;
    if !schema_files {
        write_schema_default(&schema_dir)?;
    }

    do_init(&name, &pkg, &stash_dir, &project_dir, options)
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
    Ok(hex::encode(sha1::Sha1::from(path_bytes(path)?).digest().bytes()))
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
    todo!();
    /*
    let inst = info.instance.as_ref()
        .context("remote instance is not running, cannot run migrations")?;
    let mut cmd = inst.get_command()?;
    cmd.background_for(migrate(info, false))?;
    Ok(())
    */
}

fn start(inst: &Handle) -> anyhow::Result<()> {
    todo!();
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

    println!("Applying migrations...");

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
                        eprintln!("Once service is running, \
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
            InstanceKind::Deprecated => true,
            InstanceKind::Portable(_) => true,
            InstanceKind::Remote => false,
        }
    }
}

impl Handle {
    pub fn probe(name: &str) -> anyhow::Result<Handle> {
        use crate::server::errors::InstanceNotFound;

        if let Some(info) = InstanceInfo::try_read(name)? {
            return Ok(Handle {
                name: name.into(),
                instance: InstanceKind::Portable(info),
            });
        };
        let os = crate::server::detect::current_os()?;
        let methods = os
            .get_available_methods()?
            .instantiate_all(&*os, true)?;
        let inst_info = crate::server::control::get_instance(&methods, name);
        match inst_info {
            Ok(_) => Ok(Handle {
                name: name.into(),
                instance: InstanceKind::Deprecated,
            }),
            Err(e) if e.is::<InstanceNotFound>() => Ok(Handle {
                name: name.into(),
                instance: InstanceKind::Remote,
            }),
            Err(e) => return Err(e),
        }
    }
    pub async fn get_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::uninitialized();
        builder.read_instance(&self.name).await?;
        Ok(builder)
    }
    pub async fn get_connection(&self) -> anyhow::Result<Connection> {
        Ok(self.get_builder().await?.connect().await?)
    }
    pub fn get_version(&self) -> anyhow::Result<ver::Build> {
        task::block_on(async {
            let mut conn = self.get_connection().await?;
            let ver = conn.query_row::<String, _>(r###"
                SELECT sys::get_version_str()
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
        println!("To start the server run:\n  \
                  edgedb instance start {}",
                  name.escape_default());
    } else {
        if let Some(dir) = dir_option {
            println!("To connect to {}, navigate to {} and run `edgedb`",
                name, dir.display());
        } else {
            println!("To connect to {}, run `edgedb`", name);
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
    let text = format_config(version);
    let tmp = tmp_file_path(path);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn format_config(version: &Query) -> String {
    return format!("\
        [edgedb]\n\
        server-version = {:?}\n\
    ", version.as_config_value())
}

fn ask_version(options: &Init) -> anyhow::Result<PackageInfo> {
    let ver_query = Query::from_option(&options.server_version)?;
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
