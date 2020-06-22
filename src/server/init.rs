use std::fs;
use std::io;
use std::process::Command;
use std::path::{Path, PathBuf};
use std::collections::{BTreeSet, BTreeMap};

use anyhow::Context;
use prettytable::{Table, Row, Cell};
use serde::{Serialize, Deserialize};

use crate::platform::config_dir;
use crate::server::detect::{self, VersionQuery, InstalledPackage};
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::{Init, StartConf};
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::table;


const MIN_PORT: u16 = 10700;


pub struct Settings {
    pub name: String,
    pub system: bool,
    pub version: Version<String>,
    pub method: InstallMethod,
    pub directory: PathBuf,
    pub port: u16,
    pub start_conf: StartConf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Metadata {
    pub version: Version<String>,
    pub method: InstallMethod,
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
    let port_file = config_dir()?.join("instance_ports.json");
    let mut port_map = _read_ports(&port_file).with_context(|| {
        format!("failed reading port mapping {}", port_file.display())
    })?;
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

    let mut cmd = Command::new(method.get_server_path(&settings.version)?);
    cmd.arg("--bootstrap");
    cmd.arg("--log-level=warn");
    cmd.arg("--data-dir").arg(&settings.directory);

    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
        Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
    }

    let metapath = settings.directory.join("metadata.json");
    write_metadata(&metapath, &Metadata {
        version: settings.version.clone(),
        method: settings.method.clone(),
    }).with_context(|| format!("failed to write metadata file {}",
                               metapath.display()))?;
    Ok(())
}

fn find_version<F>(methods: &Methods, mut cond: F)
    -> anyhow::Result<Option<(Version<String>, InstallMethod)>>
    where F: FnMut(&InstalledPackage) -> bool
{
    let mut max_ver = None;
    let mut ver_methods = BTreeSet::new();
    for (meth, method) in methods {
        for ver in method.installed_versions()? {
            if cond(ver) {
                if let Some(ref mut max_ver) = max_ver {
                    if *max_ver == ver.major_version {
                        ver_methods.insert(meth.clone());
                    } else if *max_ver < ver.major_version {
                        *max_ver = ver.major_version.clone();
                        ver_methods.clear();
                        ver_methods.insert(meth.clone());
                    }
                } else {
                    max_ver = Some(ver.major_version.clone());
                    ver_methods.insert(meth.clone());
                }
            }
        }
    }
    Ok(max_ver.map(|ver| (ver, ver_methods.into_iter().next().unwrap())))
}

pub fn init(options: &Init) -> anyhow::Result<()> {
    let version_query = VersionQuery::new(
        options.nightly, options.version.as_ref());
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (version, meth_name, method) = if let Some(ref meth) = options.method {
        let method = current_os.make_method(meth, &avail_methods)?;
        let mut max_ver = None;
        for ver in method.installed_versions()? {
            if let Some(ref mut max_ver) = max_ver {
                if *max_ver < ver.major_version {
                    *max_ver = ver.major_version.clone();
                }
            } else {
                max_ver = Some(ver.major_version.clone());
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
            find_version(&methods, |p| version_query.installed_matches(p))?
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
        if let Some((ver, meth_name)) = find_version(&methods, |_| true)? {
            let meth = methods.remove(&meth_name)
                .expect("method is recently used");
            (ver, meth_name, meth)
        } else {
            anyhow::bail!("Cannot find any installed version. Run: \n  \
                edgedb server install");
        }
    };
    let port = allocate_port(&options.name)?;
    let settings = Settings {
        name: options.name.clone(),
        system: options.system,
        version,
        method: meth_name,
        directory: data_path(options.system)?.join(&options.name),
        port,
        start_conf: options.start_conf,
    };
    settings.print();
    if settings.system {
        anyhow::bail!("System instances are not implemented yet"); // TODO
    } else {
        if settings.directory.exists() {
            anyhow::bail!("Directory {0} already exists. \
                This may mean that instance is already initialized. \
                Otherwise run: `rm -rf {0}` to clean up before \
                re-running `edgedb server init`.",
                settings.directory.display());
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
        match settings.start_conf {
            StartConf::Auto => {
                println!("Bootstrap complete. Server is up and runnning now.");
            }
            StartConf::Manual => {
                println!("Bootstrap complete. To start a server:\n  \
                          edgedb server start {}",
                          settings.name.escape_default());
            }
        }
        Ok(())
    }
}

fn write_metadata(path: &Path, metadata: &Metadata) -> anyhow::Result<()> {
    fs::write(path,serde_json::to_vec(&metadata)?)?;
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
            Cell::new("EdgeDB Version"),
            Cell::new(self.version.num()),
        ]));
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}
