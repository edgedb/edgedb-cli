use std::borrow::Cow;
use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use fs_err as fs;
use linked_hash_map::LinkedHashMap;
use rand::{thread_rng, Rng};

use edgedb_client::client::Connection;
use edgedb_client::Builder;

use crate::commands::ExitCode;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::platform::{tmp_file_path, config_dir, path_bytes, symlink_dir};
use crate::print;
use crate::process::ProcessGuard;
use crate::project::config;
use crate::project::options::Init;
use crate::question;
use crate::server::control::get_instance;
use crate::server::create::{self, try_bootstrap, allocate_port};
use crate::server::detect::{self, VersionQuery};
use crate::server::distribution::{DistributionRef, MajorVersion};
use crate::server::errors::InstanceNotFound;
use crate::server::install::{self, optional_docker_check, exit_codes};
use crate::server::is_valid_name;
use crate::server::methods::{InstallMethod, InstallationMethods, Methods};
use crate::server::options::{StartConf, Start};
use crate::server::os_trait::{CurrentOs, Method, InstanceRef};
use crate::server::version::Version;
use crate::table;

const DEFAULT_ESDL: &str = "\
    module default {\n\
    \n\
    }\n\
";
const WINDOWS_DOCKER_HELP: &str = "\
    You can download Docker Desktop for Windows here: \
    https://hub.docker.com/editions/community/docker-ce-desktop-windows/ \
\n";
const UNIX_DOCKER_HELP: &str = "\
    Please install Docker by following the instructions at \
    https://docs.docker.com/get-docker/\
\n";

struct InstInfo<'a> {
    name: String,
    instance: Option<InstanceRef<'a>>,
}

impl InstInfo<'_> {
    pub fn probe<'x>(methods: &'x Methods, name: &str)
        -> anyhow::Result<InstInfo<'x>>
    {
        match get_instance(methods, name) {
            Ok(inst) => Ok(InstInfo {
                name: name.into(),
                instance: Some(inst),
            }),
            Err(e) if e.is::<InstanceNotFound>() => Ok(InstInfo {
                name: name.into(),
                instance: None,
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
    pub fn get_version(&self) -> anyhow::Result<MajorVersion> {
        task::block_on(async {
            let mut conn = self.get_connection().await?;
            let (nightly, ver) = conn.query_row::<(bool, String), _>(r###"
                WITH v := sys::get_version()
                SELECT (
                    contains("dev", v.local),
                    to_str(v.major) ++ (
                        "-" ++ <str>v.stage ++ to_str(v.stage_no)
                        if v.stage != <sys::VersionStage>'final'
                        else ""
                    )
                )
            "###, &()).await?;
            if nightly {
                Ok(MajorVersion::Nightly)
            } else {
                Ok(MajorVersion::Stable(Version(ver)))
            }
        })
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

fn ask_method(available: &InstallationMethods, options: &Init)
    -> anyhow::Result<InstallMethod>
{
    if let Some(meth) = &options.server_install_method {
        return Ok(meth.clone());
    }
    if options.non_interactive {
        if available.package.supported {
            return Ok(InstallMethod::Package);
        } else if available.docker.supported {
            return Ok(InstallMethod::Docker);
        } else {
            let mut buf = String::with_capacity(1024);
            buf.push_str(
                "No installation method supported for the platform:");
            available.package.format_error(&mut buf);
            available.docker.format_error(&mut buf);
            buf.push_str("Please consider opening an issue at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
            anyhow::bail!(buf);
        }
    }
    let mut q = question::Numeric::new(
        "How would you like to run EdgeDB for this project?"
    );
    if available.package.supported {
        q.option("Local (native package)", InstallMethod::Package);
    }
    if available.docker.supported {
        q.option("Local (docker)", InstallMethod::Docker);
    }
    if q.is_empty() {
        let mut buf = String::with_capacity(1024);
        if available.docker.platform_supported {
            buf.push_str("No installation method found:\n");
            available.package.format_error(&mut buf);
            available.docker.format_error(&mut buf);
            if cfg!(windows) {
                buf.push_str("EdgeDB version installation on Windows \
                    requires Docker Desktop to be installed and running. \
                    You can download Docker Desktop for Windows here: \
                    https://hub.docker.com/editions/community/docker-ce-desktop-windows/ \
                    Once Docker Desktop is installed and running, restart \
                    the command.");
            } else {
                buf.push_str("It looks like there are no native EdgeDB version \
                    packages for your OS yet.  However, it is possible to \
                    install and run EdgeDB version in a Docker container. \
                    Please install Docker by following the instructions at \
                    https://docs.docker.com/get-docker/.  Once Docker is \
                    installed, restart the command");
            }
        } else {
            buf.push_str("No installation method supported for the platform:");
            available.package.format_error(&mut buf);
            available.docker.format_error(&mut buf);
            buf.push_str("Please consider opening an issue at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
        }
        anyhow::bail!(buf);
    }
    q.ask()
}

fn ask_name(_methods: &Methods, dir: &Path, options: &Init)
    -> anyhow::Result<(String, bool)>
{
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
                           to link project with it pass `--link` flag explicitly",
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

fn print_versions(meth: &dyn Method, title: &str) -> anyhow::Result<()> {
    let mut avail = meth.all_versions(false)?;
    avail.sort_by(|a, b| b.major_version().cmp(a.major_version()));
    println!("{}: {}{}",
        title,
        avail.iter().take(5)
            .map(|d| d.major_version().as_str().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        if avail.len() > 5 { " ..." } else { "" },
    );
    Ok(())
}

fn ask_version(meth: &dyn Method, options: &Init)
    -> anyhow::Result<DistributionRef>
{
    let ver_query = match &options.server_version {
        Some(ver) if ver.num() == "nightly" => VersionQuery::Nightly,
        Some(ver) => VersionQuery::Stable(Some(ver.clone())),
        None => VersionQuery::Stable(None),
    };
    if options.non_interactive {
        return meth.get_version(&ver_query);
    }
    let distribution = meth.get_version(&ver_query)
        .map_err(|e| {
            log::warn!("Cannot find EdgeDB {}: {}", ver_query, e);
        })
        .or_else(|()| {
            meth.get_version(&VersionQuery::Stable(None))
            .context("cannot find stable EdgeDB version")
        })?;
    let mut q = question::String::new(
        "Specify the version of EdgeDB to use with this project"
    );
    q.default(distribution.major_version().as_str());
    loop {
        let value = q.ask()?;
        let value = value.trim();
        if value == distribution.major_version().as_str() {
            return Ok(distribution);
        }
        if value == "nightly" {
            match meth.get_version(&VersionQuery::Nightly) {
                Ok(distr) => return Ok(distr),
                Err(e) => {
                    print::error(format!(
                        "Cannot find nightly version: {}", e
                    ));
                    continue;
                }
            }
        } else {
            let query = VersionQuery::Stable(Some(Version(value.into())));
            match meth.get_version(&query) {
                Ok(distr) => return Ok(distr),
                Err(e) => {
                    print::error(e);
                    print_versions(meth, "Available versions")?;
                    continue;
                }
            }
        }
    }
}

fn ask_existing_instance_name(_methods: &Methods) -> anyhow::Result<String> {
    let instances = credentials::all_instance_names()?;

    let mut q =
        question::String::new("Specify the name of EdgeDB instance to link with this project");
    loop {
        let target_name = q.ask()?;

        if instances.contains(&target_name) {
            return Ok(target_name);
        } else {
            print::error(format!("Instance {:?} doesn't exist", target_name));
        }
    }
}

pub fn init(init: &Init) -> anyhow::Result<()> {
    if optional_docker_check() {
        print::error(
            "`edgedb project init` in a Docker container is not supported.",
        );
        eprintln!("\
            To init a project run the command on the host system instead and \
            choose the `Local (docker)` installation method.");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    match &init.project_dir {
        Some(dir) => {
            let dir = fs::canonicalize(&dir)?;
            if dir.join("edgedb.toml").exists() {
                if init.link {
                    link(init, &dir)?;
                } else {
                    init_existing(init, &dir)?;
                }
            } else {
                if init.link {
                    anyhow::bail!("`edgedb.toml` was not found, unable to link an EdgeDB instance \
                                   with uninitialized project, to initialize a new project run command \
                                   without `--link` flag")
                }

                init_new(init, &dir)?;
            }
        }
        None => {
            let base_dir = env::current_dir()
                .context("failed to get current directory")?;
            if let Some(dir) = search_dir(&base_dir)? {
                let dir = fs::canonicalize(&dir)?;
                if init.link {
                    link(init, &dir)?;
                } else {
                    init_existing(init, &dir)?;
                }
            } else {
                if init.link {
                    anyhow::bail!("`edgedb.toml` was not found, unable to link an EdgeDB instance \
                                   with uninitialized project, to initialize a new project run command \
                                   without `--link` flag")
                }

                let dir = fs::canonicalize(&base_dir)?;
                init_new(init, &dir)?;
            }
        }
    };
    Ok(())
}

pub fn format_config(version: &str) -> String {
    return format!("\
        [edgedb]\n\
        server-version = {:?}\n\
    ", version)
}

#[context("cannot write config `{}`", path.display())]
fn write_config(path: &Path, version: &MajorVersion) -> anyhow::Result<()> {
    let text = format_config(version.as_str());
    let tmp = tmp_file_path(path);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
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
    let ver_query = match config.edgedb.server_version {
        None => VersionQuery::Stable(None),
        Some(ver) => ver.to_query(),
    };

    let os = detect::current_os()?;
    let methods = os
        .get_available_methods()?
        .instantiate_all(&*os, true)?;

    let name = if let Some(name) = &options.server_instance {
        name.clone()
    } else if options.non_interactive {
        anyhow::bail!("Existing instance name should be specified \
                       with `--server-instance` argument when linking project \
                       in non-interactive mode")
    } else {
        ask_existing_instance_name(&methods)?
    };

    let inst = InstInfo::probe(&methods, &name)?;
    match inst.get_version() {
        Ok(inst_ver) if ver_query.matches(&inst_ver) => {}
        Ok(inst_ver) => {
            print::warn(format!(
                "WARNING: existing instance has version {}, \
                but {} is required by `edgedb.toml`",
                inst_ver.title(), ver_query
            ));
        }
        Err(e) => {
            log::warn!("Could not check instance's version: {:#}", e);
        }
    }

    write_stash_dir(&stash_dir, &project_dir, &name)?;

    if !options.no_migrations {
        task::block_on(migrate(&inst, !options.non_interactive))?;
    }

    print::success("Project linked");
    if let Some(dir) = &options.project_dir {
        println!(
            "To connect to {}, navigate to {} and run `edgedb`",
            name,
            dir.display()
        );
    } else {
        println!("To connect to {}, run `edgedb`", name);
    }

    Ok(())
}

pub fn init_existing(options: &Init, project_dir: &Path)
    -> anyhow::Result<()>
{
    println!("Found `edgedb.toml` in `{}`", project_dir.display());
    println!("Initializing project...");

    let mut err_manual = false;
    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("Project is already initialized.");
    }

    let config_path = project_dir.join("edgedb.toml");
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;
    let config = config::read(&config_path)?;

    let ver_query = if let Some(ver) = &options.server_version {
        match ver.num() {
            "nightly" => VersionQuery::Nightly,
            _ => VersionQuery::Stable(Some(ver.clone())),
        }
    } else {
        match config.edgedb.server_version {
            None => VersionQuery::Stable(None),
            Some(ver) => ver.to_query(),
        }
    };

    let os = detect::current_os()?;
    let avail_methods = os.refresh_available_methods()?;
    let methods = avail_methods.instantiate_all(&*os, true)?;
    let (name, exists) = ask_name(&methods, project_dir, options)?;

    let inst = if exists {
        let inst = InstInfo::probe(&methods, &name)?;
        match inst.get_version() {
            Ok(inst_ver) if ver_query.matches(&inst_ver) => {}
            Ok(inst_ver) => {
                print::warn(format!(
                    "WARNING: existing instance has version {}, \
                    but {} is required by `edgedb.toml`",
                    inst_ver.title(), ver_query
                ));
            }
            Err(e) => {
                log::warn!("Could not check instance's version: {:#}", e);
            }
        }
        inst
    } else {
        let method = ask_method(&avail_methods, options)?;
        let meth = assert_method(&method, &*os, &methods, &avail_methods)?;

        println!("Checking EdgeDB versions...");

        let distr = meth.get_version(&ver_query)
            .map_err(|e| {
                print::error(format!(
                    "Cannot find EdgeDB version {}: {}", ver_query, e
                ));
                eprintln!("  Hint: try a different installation method \
                    or remove `server-version` from `edgedb.toml` to \
                    install the latest stable version.");
                ExitCode::new(1)
            })?;

        let installed = meth.installed_versions()?;

        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                if schema_files { "(non-empty)" } else { "(empty)" }),
                &schema_dir.display().to_string()),
            ("Installation method", method.title()),
            ("Version", distr.version().as_ref()),
            ("Instance name", &name),
        ]);
        // TODO(tailhook) this condition doesn't work for nightly
        if !installed.iter()
            .any(|x| x.major_version() == distr.major_version())
        {
            println!("Installing EdgeDB version {}...",
                     distr.major_version().title());
            meth.install(&install::Settings {
                method: method.clone(),
                distribution: distr.clone(),
                extra: LinkedHashMap::new(),
            })?;
        }

        write_config(&config_path, distr.major_version())?;
        if !schema_files {
            write_default(&schema_dir)?;
        }

        let settings = create::Settings {
            name: name.clone(),
            system: false,
            version: distr.version().clone(),
            nightly: distr.major_version().is_nightly(),
            distribution: distr,
            method: method,
            storage: meth.get_storage(false, &name)?,
            credentials: credentials::path(&name)?,
            user: "edgedb".into(),
            database: "edgedb".into(),
            port: allocate_port(&name)?,
            start_conf: StartConf::Auto,
            suppress_messages: true,
        };

        println!("Initializing EdgeDB instance...");
        if !try_bootstrap(meth, &settings)? {
            err_manual = true;
        }
        InstInfo::probe(&methods, &name)?
    };

    write_stash_dir(&stash_dir, project_dir, &name)?;

    if err_manual {
        if !options.no_migrations {
            run_and_migrate(&inst)?;
        }
        print::error("Bootstrapping complete, \
            but there was an error creating the service.");
        eprintln!("You can start it manually via: \n  \
            edgedb instance start --foreground {}",
            name.escape_default());
        return Err(ExitCode::new(2))?;
    } else {
        if !options.no_migrations {
            task::block_on(migrate(&inst,
                                   exists && !options.non_interactive))?;
        }
        print_initialized(&name, &options.project_dir);
    }

    Ok(())
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

#[context("cannot create default schema in `{}`", dir.display())]
fn write_default(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(&dir)?;
    fs::create_dir_all(&dir.join("migrations"))?;
    let default = dir.join("default.esdl");
    let tmp = tmp_file_path(&default);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, DEFAULT_ESDL)?;
    fs::rename(&tmp, &default)?;
    Ok(())
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

pub fn stash_base() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

pub fn stash_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let hname = stash_name(project_dir)?;
    Ok(stash_base()?.join(hname))
}

pub fn assert_method<'x: 'y, 'y>(method: &InstallMethod,
    os: &'x dyn CurrentOs,
    methods: &'y Methods<'x>, available: &InstallationMethods)
    -> anyhow::Result<&'y (dyn Method + 'x)>
{
    if let Some(meth) = methods.get(&method) {
        Ok(meth.as_ref())
    } else {
        let mut buf = String::with_capacity(1024);
        match method {
            InstallMethod::Docker => {
                available.docker.format_error(&mut buf);
                if available.docker.platform_supported {
                    if cfg!(windows) {
                        buf.push_str(WINDOWS_DOCKER_HELP);
                    } else {
                        buf.push_str(UNIX_DOCKER_HELP);
                    }
                }
            }
            InstallMethod::Package => {
                available.package.format_error(&mut buf);
            }
        }
        // This should error out and show the error,
        let e = os.make_method(&method, &available)
            .expect_err("make method worked the second time");
        eprint!("{}", buf);
        return Err(e);
    }
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

    let mut err_manual = false;

    let config_path = project_dir.join("edgedb.toml");
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let methods = avail_methods.instantiate_all(&*os, true)?;
    let (name, exists) = ask_name(&methods, project_dir, options)?;

    let inst = if exists {
        let inst = InstInfo::probe(&methods, &name)?;

        write_config(&config_path, &inst.get_version()?)?;
        if !schema_files {
            write_default(&schema_dir)?;
        }

        inst
    } else {
        let method = ask_method(&avail_methods, options)?;
        let meth = assert_method(&method, &*os, &methods, &avail_methods)?;

        println!("Checking EdgeDB versions...");
        let installed = meth.installed_versions()?;

        let distr = ask_version(meth, options)?;

        table::settings(&[
            ("Project directory", &project_dir.display().to_string()),
            ("Project config", &config_path.display().to_string()),
            (&format!("Schema dir {}",
                if schema_files { "(non-empty)" } else { "(empty)" }),
                &schema_dir.display().to_string()),
            ("Installation method", method.title()),
            ("Version", distr.version().as_ref()),
            ("Instance name", &name),
        ]);

        // TODO(tailhook) this condition doesn't work for nightly
        if !installed.iter()
            .any(|x| x.major_version() == distr.major_version()) {
            println!("Installing EdgeDB version {}...",
                     distr.major_version().title());
            meth.install(&install::Settings {
                method: method.clone(),
                distribution: distr.clone(),
                extra: LinkedHashMap::new(),
            })?;
        }

        write_config(&config_path, distr.major_version())?;
        if !schema_files {
            write_default(&schema_dir)?;
        }
        let settings = create::Settings {
            name: name.clone(),
            system: false,
            version: distr.version().clone(),
            nightly: distr.major_version().is_nightly(),
            distribution: distr,
            method: method,
            storage: meth.get_storage(false, &name)?,
            credentials: credentials::path(&name)?,
            user: "edgedb".into(),
            database: "edgedb".into(),
            port: allocate_port(&name)?,
            start_conf: StartConf::Auto,
            suppress_messages: true,
        };

        println!("Initializing EdgeDB instance...");
        if !try_bootstrap(meth, &settings)? {
            err_manual = true;
        }

        InstInfo::probe(&methods, &name)?
    };

    write_stash_dir(&stash_dir, project_dir, &name)?;

    if err_manual {
        if !options.no_migrations {
            run_and_migrate(&inst)?;
        }
        print::error("Bootstrapping complete, \
            but there was an error creating the service.");
        eprintln!("You can start it manually via: \n  \
            edgedb instance start --foreground {}",
            name.escape_default());
        return Err(ExitCode::new(2))?;
    } else {
        if !options.no_migrations {
            task::block_on(migrate(&inst,
                                   exists && !options.non_interactive))?;
        }
        print_initialized(&name, &options.project_dir);
    }

    Ok(())
}

fn print_initialized(name: &str, dir_option: &Option<PathBuf>) {
    print::success("Project initialized.");
    if let Some(dir) = dir_option {
        println!("To connect to {}, navigate to {} and run `edgedb`",
            name, dir.display());
    } else {
        println!("To connect to {}, run `edgedb`", name);
    }
}

fn run_and_migrate(info: &InstInfo) -> anyhow::Result<()> {
    let inst = info.instance.as_ref()
        .context("remote instance is not running, cannot run migrations")?;
    let mut cmd = inst.get_command()?;
    log::info!("Running server manually: {:?}", cmd);
    let child = ProcessGuard::run(&mut cmd)
        .with_context(|| format!("error running server {:?}", cmd))?;
    task::block_on(migrate(info, false))?;
    drop(child);
    Ok(())
}

async fn migrate(inst: &InstInfo<'_>, ask_for_running: bool)
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
            Err(e) if ask_for_running && inst.instance.is_some() => {
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
                    Service => match
                        inst.instance.as_ref().unwrap().start(&Start {
                            name: inst.name.clone(),
                            foreground: false,
                        })
                    {
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

#[test]
fn test_stash_name() {
    assert_eq!(
        stash_name(&Path::new("/home/user/work/project1")).unwrap(),
        "project1-cf1c841351bf7f147d70dcb6203441cf77a05249",
    );
}
