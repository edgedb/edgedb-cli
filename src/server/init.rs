use std::collections::{BTreeSet, BTreeMap};
use std::default::Default;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use async_std::task;
use edgeql_parser::helpers::{quote_string, quote_name};
use prettytable::{Table, Row, Cell};
use fn_error_context::context;

use crate::platform::{ProcessGuard, config_dir, home_dir};
use crate::server::control;
use crate::server::reset_password::{generate_password, write_credentials};
use crate::server::detect::{self, VersionQuery};
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::{Init, Start, StartConf};
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::server::distribution::DistributionRef;
use crate::server::package::Package;
use crate::table;

use edgedb_client::credentials::Credentials;


const MIN_PORT: u16 = 10700;


pub struct Settings {
    pub name: String,
    pub system: bool,
    pub distribution: DistributionRef,
    pub version: Version<String>,
    pub nightly: bool,
    pub method: InstallMethod,
    pub directory: PathBuf,
    pub credentials: PathBuf,
    pub user: String,
    pub database: String,
    pub port: u16,
    pub start_conf: StartConf,
    pub inhibit_user_creation: bool,
    pub inhibit_start: bool,
    pub upgrade_marker: Option<String>,
}

pub fn data_path(system: bool) -> anyhow::Result<PathBuf> {
    if system {
        anyhow::bail!("System instances are not implemented yet"); // TODO
    } else {
        Ok(dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Can't determine data directory"))?
            .join("edgedb/data"))
    }
}

fn port_file() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("instance_ports.json"))
}

pub fn read_ports() -> anyhow::Result<BTreeMap<String, u16>> {
    _read_ports(&port_file()?)
}

#[context("failed reading port mapping {}", path.display())]
fn _read_ports(path: &Path) -> anyhow::Result<BTreeMap<String, u16>> {
    let data = match fs::read_to_string(&path) {
        Ok(data) if data.is_empty() => {
            return Ok(BTreeMap::new());
        }
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(e) => return Err(e)?,
    };
    Ok(serde_json::from_str(&data)?)
}

fn next_min_port(port_map: &BTreeMap<String, u16>) -> u16 {
    if port_map.len() == 0 {
        return MIN_PORT;
    }
    let port_set: BTreeSet<u16> = port_map.values().cloned().collect();
    let mut prev = MIN_PORT - 1;
    for port in port_set {
        if port > prev+1 {
            return prev + 1;
        }
        prev = port;
    }
    return prev+1;
}

fn _write_ports(port_map: &BTreeMap<String, u16>, port_file: &Path)
    -> anyhow::Result<()>
{
    let config_dir = config_dir()?;
    fs::create_dir_all(&config_dir)?;
    let tmp_file = config_dir.join(".instance_ports.json.tmp");
    fs::remove_file(&tmp_file).ok();
    serde_json::to_writer_pretty(fs::File::create(&tmp_file)?, &port_map)?;
    fs::rename(&tmp_file, &port_file)?;
    Ok(())
}

fn allocate_port(name: &str) -> anyhow::Result<u16> {
    let port_file = port_file()?;
    let mut port_map = _read_ports(&port_file)?;
    if let Some(port) = port_map.get(name) {
        return Ok(*port);
    }
    if name == "default" {
        return Ok(5656);
    }
    let port = next_min_port(&port_map);
    port_map.insert(name.to_string(), port);
    _write_ports(&port_map, &port_file).with_context(|| {
        format!("failed writing port mapping {}", port_file.display())
    })?;
    Ok(port)
}

fn try_bootstrap(settings: &Settings, method: &dyn Method)
    -> anyhow::Result<()>
{
    fs::create_dir_all(&settings.directory)
        .with_context(|| format!("failed to create {}",
                                 settings.directory.display()))?;

    let mut cmd = Command::new(
        method.get_server_path(&settings.distribution)?);
    cmd.arg("--bootstrap");
    cmd.arg("--log-level=warn");
    cmd.arg("--data-dir").arg(&settings.directory);
    if settings.inhibit_user_creation {
        cmd.arg("--default-database=edgedb");
        cmd.arg("--default-database-user=edgedb");
    }

    log::debug!("Running bootstrap {:?}", cmd);
    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
        Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
    }

    if let Some(upgrade_marker) = &settings.upgrade_marker {
        write_upgrade(
            &settings.directory.join("UPGRADE_IN_PROGRESS"),
            upgrade_marker)?;
    }

    let metapath = settings.directory.join("metadata.json");
    write_metadata(&metapath, &Metadata {
        version: settings.distribution.major_version().clone(),
        current_version: Some(settings.distribution.version().clone()),
        slot: settings.distribution.downcast_ref::<Package>()
            .map(|p| p.slot.clone()),
        method: settings.method.clone(),
        port: settings.port,
        start_conf: settings.start_conf,
    })?;
    Ok(())
}

fn find_version<F>(methods: &Methods, mut cond: F)
    -> anyhow::Result<Option<(DistributionRef, InstallMethod)>>
    where F: FnMut(&DistributionRef) -> bool
{
    let mut max_ver = None::<DistributionRef>;
    let mut ver_methods = BTreeSet::new();
    for (meth, method) in methods {
        for distr in method.installed_versions()? {
            if cond(&distr) {
                if let Some(ref mut max_ver) = max_ver {
                    if max_ver.major_version() == distr.major_version() {
                        if max_ver.version() < distr.version() {
                            *max_ver = distr;
                        }
                        ver_methods.insert(meth.clone());
                    } else if max_ver.major_version() < distr.major_version() {
                        *max_ver = distr;
                        ver_methods.clear();
                        ver_methods.insert(meth.clone());
                    }
                } else {
                    max_ver = Some(distr);
                    ver_methods.insert(meth.clone());
                }
            }
        }
    }
    Ok(max_ver.map(|distr| (distr, ver_methods.into_iter().next().unwrap())))
}

pub fn init(options: &Init) -> anyhow::Result<()> {
    let version_query = VersionQuery::new(
        options.nightly, options.version.as_ref());
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (distr, meth_name, method) = if let Some(ref meth) = options.method {
        let method = current_os.make_method(meth, &avail_methods)?;
        let mut max_ver = None::<DistributionRef>;
        for distr in method.installed_versions()? {
            if let Some(ref mut max_ver) = max_ver {
                if (max_ver.major_version(), max_ver.version()) <
                    (distr.major_version(), max_ver.version()) {
                    *max_ver = distr;
                }
            } else {
                max_ver = Some(distr);
            }
        }
        if let Some(ver) = max_ver {
            (ver, meth.clone(), method)
        } else {
            anyhow::bail!("Cannot find any installed version. Run: \n  \
                edgedb server install {}", meth.option());
        }
    } else if version_query.is_nightly() || version_query.is_specific() {
        let mut methods = avail_methods.instantiate_all(&*current_os, true)?;
        if let Some((ver, meth_name)) =
            find_version(&methods, |p| version_query.distribution_matches(p))?
        {
            let meth = methods.remove(&meth_name)
                .expect("method is recently used");
            (ver, meth_name, meth)
        } else {
            anyhow::bail!("Cannot find version {} installed. Run: \n  \
                edgedb server install {}",
                version_query,
                version_query.to_arg().unwrap_or_else(String::new));
        }

    } else {
        let mut methods = avail_methods.instantiate_all(&*current_os, true)?;
        if let Some((ver, meth_name)) =
            find_version(&methods, |p| !p.major_version().is_nightly())?
        {
            let meth = methods.remove(&meth_name)
                .expect("method is recently used");
            (ver, meth_name, meth)
        } else {
            anyhow::bail!("Cannot find any installed version \
                (note: nightly versions are skipped unless `--nightly` \
                is used).\nRun: \n  \
                edgedb server install");
        }
    };
    let port = allocate_port(&options.name)?;
    let settings = Settings {
        name: options.name.clone(),
        system: options.system,
        version: distr.version().clone(),
        distribution: distr,
        nightly: version_query.is_nightly(),
        method: meth_name,
        directory: data_path(options.system)?.join(&options.name),
        credentials: home_dir()?.join(".edgedb").join("credentials")
            .join(format!("{}.json", &options.name)),
        user: options.default_user.clone(),
        database: options.default_database.clone(),
        port,
        start_conf: options.start_conf,
        inhibit_user_creation: options.inhibit_user_creation,
        inhibit_start: options.inhibit_start,
        upgrade_marker: options.upgrade_marker.clone(),
    };
    settings.print();
    if settings.system {
        anyhow::bail!("System instances are not implemented yet"); // TODO
    } else {
        if settings.credentials.exists() && !options.overwrite {
            anyhow::bail!("Credential file {0} already exists. \
                This may mean that instance is already initialized. \
                You may run `--overwrite` to overwrite the instance.",
                settings.credentials.display());
        }
        if settings.directory.exists() {
            if options.overwrite {
                fs::remove_dir_all(&settings.directory)
                    .with_context(|| format!("cannot remove previous \
                        instance directory {}",
                        settings.directory.display()))?;
            } else {
                anyhow::bail!("Directory {0} already exists. \
                    This may mean that instance is already initialized. \
                    Otherwise run: `rm -rf {0}` to clean up before \
                    re-running `edgedb server init`.",
                    settings.directory.display());
            }
        }
        match try_bootstrap(&settings, &*method) {
            Ok(()) => {}
            Err(e) => {
                log::error!("Bootstrap error, cleaning up...");
                fs::remove_dir_all(&settings.directory)
                    .with_context(|| format!("failed to clean up {}",
                                             settings.directory.display()))?;
                Err(e).context(format!("Error bootstrapping {}",
                                       settings.directory.display()))?
            }
        }
        method.create_user_service(&settings).map_err(|e| {
            eprintln!("Bootrapping complete, \
                but there was an error creating a service. \
                You can run server manually via: \n  \
                edgedb server start --foreground {}",
                settings.name.escape_default());
            e
        }).context("failed to init service")?;
        match (settings.start_conf, settings.inhibit_start) {
            (StartConf::Auto, false) => {
                let mut inst = control::get_instance(&settings.name)?;
                inst.start(&Start {
                        name: settings.name.clone(),
                        foreground: false,
                    })?;
                init_credentials(&settings, &*inst)?;
                println!("Bootstrap complete. Server is up and runnning now.");
            }
            (StartConf::Manual, _) | (_, true) => {
                let inst = control::get_instance(&settings.name)?;
                let mut cmd = inst.run_command()?;
                log::debug!("Running server: {:?}", cmd);
                let child = ProcessGuard::run(&mut cmd)
                    .with_context(||
                        format!("error running server {:?}", cmd))?;
                init_credentials(&settings, &*inst)?;
                drop(child);
                println!("Bootstrap complete. To start a server:\n  \
                          edgedb server start {}",
                          settings.name.escape_default());
            }
        }
        Ok(())
    }
}

fn init_credentials(settings: &Settings, inst: &dyn control::Instance)
    -> anyhow::Result<()>
{
    let password = generate_password();

    let mut conn_params = edgedb_client::Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(inst.get_socket(true)?);
    conn_params.wait_until_available(Duration::from_secs(30));
    task::block_on(async {
        let mut cli = conn_params.connect().await?;
        if settings.database != "edgedb" {
            cli.execute(
                &format!("CREATE DATABASE {}", quote_name(&settings.database))
            ).await?;
        }
        if settings.user == "edgedb" {
            cli.execute(&format!(r###"
                ALTER ROLE {name} {{
                    SET password := {password};
                }}"###,
                name=quote_name(&settings.user),
                password=quote_string(&password))
            ).await
        } else {
            cli.execute(&format!(r###"
                CREATE SUPERUSER ROLE {name} {{
                    SET password := {password};
                }}"###,
                name=quote_name(&settings.user),
                password=quote_string(&password))
            ).await
        }
    })?;

    let mut creds = Credentials::default();
    creds.port = settings.port;
    creds.user = settings.user.clone();
    creds.database = Some(settings.database.clone());
    creds.password = Some(password);
    write_credentials(&settings.credentials, &creds)?;
    Ok(())
}

#[context("failed to write upgrade marker {}", path.display())]
fn write_upgrade(path: &Path, data: &str) -> anyhow::Result<()> {
    fs::write(path, data.as_bytes())?;
    Ok(())
}

#[context("failed to write metadata file {}", path.display())]
fn write_metadata(path: &Path, metadata: &Metadata) -> anyhow::Result<()> {
    fs::write(path, serde_json::to_vec_pretty(&metadata)?)?;
    Ok(())
}

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Instance Name"),
            Cell::new(&self.name),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Mode"),
            Cell::new(if self.method == InstallMethod::Docker {
                "Docker"
            } else if self.system {
                "System Service"
            } else {
                "User Service"
            }),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Data Directory"),
            Cell::new(&self.directory.display().to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Credentials Path"),
            Cell::new(&self.credentials.display().to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Database Server Port"),
            Cell::new(&self.port.to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Default User"),
            Cell::new(&self.user.to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Default Database"),
            Cell::new(&self.database.to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("EdgeDB Version"),
            Cell::new(&if self.nightly {
                format!("{} (nightly)", self.version)
            } else {
                self.version.to_string()
            }),
        ]));
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}
