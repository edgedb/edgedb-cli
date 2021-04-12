use std::collections::BTreeSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use fn_error_context::context;
use linked_hash_map::LinkedHashMap;
use rand::{thread_rng, seq::SliceRandom};

use crate::commands::ExitCode;
use crate::platform::{tmp_file_path, home_dir, path_bytes, symlink_dir};
use crate::project::options::Init;
use crate::question;
use crate::server::detect::{self, VersionQuery};
use crate::server::distribution::DistributionRef;
use crate::server::init::{self, try_bootstrap, allocate_port};
use crate::server::options::{StartConf};
use crate::server::install::{self, optional_docker_check, exit_codes};
use crate::server::methods::{InstallMethod, InstallationMethods, Methods};
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::table;
use crate::project::config;

const CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
const DEFAULT_ESDL: &str = r#"
module default {

}
"#;


pub fn config_dir(base: &Path) -> anyhow::Result<Option<PathBuf>> {
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

fn ask_method(available: &InstallationMethods) -> anyhow::Result<InstallMethod>
{
    let mut q = question::Numeric::new(
        "What type of EdgeDB instance would you like to use with this project?"
    );
    if available.package.supported {
        q.option("Local (native package)", InstallMethod::Package);
    }
    if available.package.supported {
        q.option("Local (docker)", InstallMethod::Docker);
    }
    q.ask()
}

fn ask_name(methods: &Methods, dir: &Path) -> anyhow::Result<String> {
    let instances = methods.values()
        .map(|m| m.all_instances())
        .collect::<Result<Vec<_>, _>>()
        .context("failed to enumerate existing instances")?
        .into_iter().flatten()
        .map(|inst| inst.name().to_string())
        .collect::<BTreeSet<_>>();
    let stem = dir.file_stem().and_then(|s| s.to_str()).unwrap_or("edgedb");
    let mut name = stem.to_string();

    while instances.contains(&name) {
        name = format!("{}_{}", stem,
            (0..7)
            .flat_map(|_| CHARS.as_bytes().choose(&mut thread_rng()))
            .map(|b| *b as char)
            .collect::<String>());
    }
    let mut q = question::String::new(
        "Specify the name of EdgeDB instance to use with this project"
    );
    q.default(&name);
    loop {
        let target_name = q.ask()?;
        if instances.contains(&target_name) {
            let confirm = question::Confirm::new(
                format!("Do you want to use existing instance {:?} \
                         for the project?",
                         target_name)
            );
            if confirm.ask()? {
                return Ok(target_name);
            }
        } else {
            return Ok(target_name)
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

fn ask_version(meth: &dyn Method) -> anyhow::Result<DistributionRef> {
    let distribution = meth.get_version(&VersionQuery::Stable(None))
        .context("cannot find stable EdgeDB version")?;
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
                    eprintln!("Cannot find nightly version: {}", e);
                    continue;
                }
            }
        } else {
            let query = VersionQuery::Stable(Some(Version(value.into())));
            match meth.get_version(&query) {
                Ok(distr) => return Ok(distr),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    print_versions(meth, "Available versions")?;
                    continue;
                }
            }
        }
    }
}

pub fn init(init: &Init) -> anyhow::Result<()> {
    if optional_docker_check() {
        eprintln!("edgedb error: \
            `edgedb project init` in a Docker container is not supported.\n\
            To init project run the command on the host system instead and \
            choose `Local (docker)` installation method.");
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    let dir = match &init.project_dir {
        Some(dir) => dir.clone(),
        None => env::current_dir()
            .context("failed to get current directory")?,
    };
    if let Some(dir) = config_dir(&dir)? {
        let dir = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        init_existing(init, &dir)?;
    } else {
        let dir = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        init_new(init, &dir)?;
    }
    Ok(())
}

#[context("cannot write config `{}`", path.display())]
fn write_config(path: &Path, distr: &DistributionRef) -> anyhow::Result<()> {
    let text = format!(r#"
[edgedb]
server-version = {:?}
"#, distr.major_version().as_str());
    let tmp = tmp_file_path(path);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn init_existing(_init: &Init, project_dir: &Path) -> anyhow::Result<()> {
    println!("Found `edgedb.toml` in `{}`", project_dir.display());

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("project dir already exists");
    }

    let config_path = project_dir.join("edgedb.toml");
    let config = config::read(&config_path)?;

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let methods = avail_methods.instantiate_all(&*os, true)?;

    let method = ask_method(&avail_methods)?;
    let meth = methods.get(&method).expect("chosen method works");

    let ver_query = match config.edgedb.server_version {
        None => VersionQuery::Stable(None),
        Some(ver) => ver.to_query(),
    };
    let distr = meth.get_version(&ver_query)
        .map_err(|e| {
            eprintln!("edgedb error: \
                Cannot find EdgeDB version {}: {}", ver_query, e);
            eprintln!("  Hint: try different installation method \
                or remove `server-version` from `edgedb.toml` to \
                install the latest stable");
            ExitCode::new(1)
        })?;

    let installed = meth.installed_versions()?;
    let name = ask_name(&methods, project_dir)?;
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;

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
    if !installed.iter().any(|x| x.major_version() == distr.major_version()) {
        meth.install(&install::Settings {
            method: method.clone(),
            distribution: distr.clone(),
            extra: LinkedHashMap::new(),
        })?;
    }

    write_config(&config_path, &distr)?;
    if !schema_files {
        write_default(&schema_dir)?;
    }

    let settings = init::Settings {
        name: name.clone(),
        system: false,
        version: distr.version().clone(),
        nightly: distr.major_version().is_nightly(),
        distribution: distr,
        method: method,
        storage: meth.get_storage(false, &name)?,
        credentials: home_dir()?.join(".edgedb").join("credentials")
            .join(format!("{}.json", &name)),
        user: "edgedb".into(),
        database: "edgedb".into(),
        port: allocate_port(&name)?,
        start_conf: StartConf::Manual,
    };

    let err_manual = !try_bootstrap(meth.as_ref(), &settings)?;
    // TODO(tailhook) execute migrations

    write_stash_dir(&stash_dir, project_dir, &name)?;

    if err_manual {
        eprintln!("Bootstrapping complete, \
            but there was an error creating the service. \
            You can run server manually via: \n  \
            edgedb server start --foreground {}",
            settings.name.escape_default());
        return Err(ExitCode::new(2))?;
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
            log::warn!("Error symlinking project at {:?}: {}", lnk, e);
        }).ok();
    fs::rename(&tmp, dir)?;
    Ok(())
}

pub fn stash_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let hname = stash_name(project_dir)?;
    Ok(home_dir()?.join(".edgedb").join("projects").join(hname))
}

fn init_new(_init: &Init, project_dir: &Path) -> anyhow::Result<()> {
    println!("`edgedb.toml` is not found in `{}` or above",
             project_dir.display());

    let stash_dir = stash_path(project_dir)?;
    if stash_dir.exists() {
        // TODO(tailhook) do more checks and probably cleanup the dir
        anyhow::bail!("project dir already exists");
    }

    let q = question::Confirm::new("Do you want to initialize a new project?");
    if !q.ask()? {
        return Ok(());
    }

    let config_path = project_dir.join("edgedb.toml");

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let methods = avail_methods.instantiate_all(&*os, true)?;

    let method = ask_method(&avail_methods)?;
    let meth = methods.get(&method).expect("chosen method works");
    let installed = meth.installed_versions()?;
    let distr = ask_version(meth.as_ref())?;
    let name = ask_name(&methods, project_dir)?;
    let schema_dir = project_dir.join("dbschema");
    let schema_files = find_schema_files(&schema_dir)?;

    table::settings(&[
        ("Project directory", &project_dir.display().to_string()),
        ("Project config", &config_path.display().to_string()),
        (&format!("Schema dir {}",
            if schema_files { "(non-empty)" } else { "(empty)" }),
            &config_path.display().to_string()),
        ("Installation method", method.title()),
        ("Version", distr.version().as_ref()),
        ("Instance name", &name),
    ]);

    // TODO(tailhook) this condition doesn't work for nightly
    if !installed.iter().any(|x| x.major_version() == distr.major_version()) {
        meth.install(&install::Settings {
            method: method.clone(),
            distribution: distr.clone(),
            extra: LinkedHashMap::new(),
        })?;
    }

    write_config(&config_path, &distr)?;
    if !schema_files {
        write_default(&schema_dir)?;
    }

    let settings = init::Settings {
        name: name.clone(),
        system: false,
        version: distr.version().clone(),
        nightly: distr.major_version().is_nightly(),
        distribution: distr,
        method: method,
        storage: meth.get_storage(false, &name)?,
        credentials: home_dir()?.join(".edgedb").join("credentials")
            .join(format!("{}.json", &name)),
        user: "edgedb".into(),
        database: "edgedb".into(),
        port: allocate_port(&name)?,
        start_conf: StartConf::Manual,
    };

    let err_manual = !try_bootstrap(meth.as_ref(), &settings)?;
    // TODO(tailhook) execute migrations

    write_stash_dir(&stash_dir, project_dir, &name)?;

    if err_manual {
        eprintln!("Bootstrapping complete, \
            but there was an error creating the service. \
            You can run server manually via: \n  \
            edgedb server start --foreground {}",
            settings.name.escape_default());
        return Err(ExitCode::new(2))?;
    }

    Ok(())
}
