use std::collections::{BTreeSet, BTreeMap};
use std::default::Default;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use async_std::task;
use edgeql_parser::helpers::{quote_string, quote_name};
use prettytable::{Table, Row, Cell};
use fn_error_context::context;

use crate::commands::ExitCode;
use crate::platform::{config_dir, home_dir};
use crate::server::reset_password::{generate_password, write_credentials};
use crate::server::reset_password::{password_hash};
use crate::server::detect::{self, VersionQuery};
use crate::server::errors::CannotCreateService;
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::{Init, StartConf};
use crate::server::os_trait::{InstanceRef};
use crate::server::version::Version;
use crate::server::distribution::DistributionRef;
use crate::server::package::Package;
use crate::table;

use edgedb_client::credentials::Credentials;


const MIN_PORT: u16 = 10700;


#[derive(Clone, Debug)]
pub enum Storage {
    UserDir(PathBuf),
    DockerVolume(String),
}

pub struct StorageDisplay<'a>(&'a Storage);

pub struct Settings {
    pub name: String,
    pub system: bool,
    pub distribution: DistributionRef,
    pub version: Version<String>,
    pub nightly: bool,
    pub method: InstallMethod,
    pub storage: Storage,
    pub credentials: PathBuf,
    pub user: String,
    pub database: String,
    pub port: u16,
    pub start_conf: StartConf,
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
    let port = next_min_port(&port_map);
    port_map.insert(name.to_string(), port);
    _write_ports(&port_map, &port_file).with_context(|| {
        format!("failed writing port mapping {}", port_file.display())
    })?;
    Ok(port)
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
        storage: method.get_storage(options.system, &options.name)?,
        credentials: home_dir()?.join(".edgedb").join("credentials")
            .join(format!("{}.json", &options.name)),
        user: options.default_user.clone(),
        database: options.default_database.clone(),
        port,
        start_conf: options.start_conf,
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
        if method.storage_exists(&settings.storage)? {
            if options.overwrite {
                method.clean_storage(&settings.storage)
                    .with_context(|| {
                        format!("can't clean previous storage directory {}",
                                settings.storage.display())
                    })?;
            } else {
                anyhow::bail!("Storage {} already exists. \
                    This may mean that instance is already \
                    initialized. Otherwise rerun with \
                    `--overwrite` flag.",
                    settings.storage.display());
            }
        }
        match method.bootstrap(&settings) {
            Ok(()) => {}
            Err(e) => {
                if e.is::<CannotCreateService>() {
                    eprintln!("edgedb error: {:#}", e);
                    eprintln!("Bootstrapping complete, \
                        but there was an error creating the service. \
                        You can run server manually via: \n  \
                        edgedb server start --foreground {}",
                        settings.name.escape_default());
                    return Err(ExitCode::new(2))?;
                } else {
                    log::error!("Bootstrap error, cleaning up...");
                    method.clean_storage(&settings.storage)
                        .map_err(|e| {
                            log::error!("Cannot clean up storage {}: {}",
                                settings.storage.display(), e);
                        }).ok();
                    Err(e).context(format!("cannot bootstrap {}",
                                           settings.storage.display()))?
                }
            }
        }
        Ok(())
    }
}

pub fn bootstrap_script(settings: &Settings, password: &str) -> String {
    use std::fmt::Write;

    let mut output = String::with_capacity(1024);
    if settings.database != "edgedb" {
        writeln!(&mut output,
            "CREATE DATABASE {};",
            quote_name(&settings.database),
        ).unwrap();
    }
    if settings.user == "edgedb" {
        writeln!(&mut output, r###"
            ALTER ROLE {name} {{
                SET password_hash := {password_hash};
            }};
            "###,
            name=quote_name(&settings.user),
            password_hash=quote_string(&password_hash(password)),
        ).unwrap();
    } else {
        writeln!(&mut output, r###"
            CREATE SUPERUSER ROLE {name} {{
                SET password_hash := {password_hash};
            }}"###,
            name=quote_name(&settings.user),
            password_hash=quote_string(&password_hash(password)),
        ).unwrap();
    }
    return output;
}

pub fn save_credentials(settings: &Settings, password: &str)
    -> anyhow::Result<()>
{
    let mut creds = Credentials::default();
    creds.port = settings.port;
    creds.user = settings.user.clone();
    creds.database = Some(settings.database.clone());
    creds.password = Some(password.into());
    write_credentials(&settings.credentials, &creds)?;
    Ok(())
}

pub fn init_credentials(settings: &Settings, inst: &InstanceRef)
    -> anyhow::Result<()>
{
    let password = generate_password();

    let mut conn_params = inst.get_connector(true)?;
    conn_params.wait_until_available(Duration::from_secs(30));
    task::block_on(async {
        let mut cli = conn_params.connect().await?;
        cli.execute(&bootstrap_script(settings, &password)).await?;
        Ok::<(), anyhow::Error>(())
    })?;

    save_credentials(settings, &password)?;
    Ok(())
}

impl Settings {
    pub fn metadata(&self) -> Metadata {
        Metadata {
            version: self.distribution.major_version().clone(),
            current_version: Some(self.distribution.version().clone()),
            slot: self.distribution.downcast_ref::<Package>()
                .map(|p| p.slot.clone()),
            method: self.method.clone(),
            port: self.port,
            start_conf: self.start_conf,
        }
    }
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
            Cell::new(&self.storage.display().to_string()),
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

impl Storage {
    pub fn display(&self) -> StorageDisplay {
        StorageDisplay(self)
    }
}

impl fmt::Display for StorageDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Storage::*;
        match self.0 {
            UserDir(path) => path.display().fmt(f),
            DockerVolume(name) => write!(f, "<docker volume {:?}>", name),
        }
    }
}
