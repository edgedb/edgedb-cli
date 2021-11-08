use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_std::task;
use fn_error_context::context;

use edgedb_client::client::Connection;
use edgedb_client::Builder;

use crate::commands::ExitCode;
use crate::connect::Connector;
use crate::credentials;
use crate::migrations;
use crate::platform::{tmp_file_path, path_bytes, symlink_dir, config_dir};
use crate::portable::config;
use crate::portable::create::InstanceInfo;
use crate::portable::exit_codes;
use crate::portable::platform::optional_docker_check;
use crate::portable::ver;
use crate::print;
use crate::project::options::Init;
use crate::question;


pub struct InstInfo {
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

    let inst = InstInfo::probe(&name)?;
    match inst.get_version() {
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
    todo!();
}

pub fn init_new(options: &Init, project_dir: &Path) -> anyhow::Result<()> {
    eprintln!("No `edgedb.toml` found in `{}` or above",
              project_dir.display());
    todo!();
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

fn run_and_migrate(info: &InstInfo) -> anyhow::Result<()> {
    todo!();
    /*
    let inst = info.instance.as_ref()
        .context("remote instance is not running, cannot run migrations")?;
    let mut cmd = inst.get_command()?;
    cmd.background_for(migrate(info, false))?;
    Ok(())
    */
}

fn start(inst: &InstInfo) -> anyhow::Result<()> {
    todo!();
}

async fn migrate(inst: &InstInfo, ask_for_running: bool)
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

impl InstInfo {
    pub fn probe(name: &str) -> anyhow::Result<InstInfo> {
        use crate::server::errors::InstanceNotFound;

        if let Some(info) = InstanceInfo::try_read(name)? {
            return Ok(InstInfo {
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
            Ok(_) => Ok(InstInfo {
                name: name.into(),
                instance: InstanceKind::Deprecated,
            }),
            Err(e) if e.is::<InstanceNotFound>() => Ok(InstInfo {
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
}
