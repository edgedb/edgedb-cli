use std::fs;
use std::process::Command;
use std::path::{Path, PathBuf};
use std::collections::BTreeSet;

use anyhow::Context;
use prettytable::{Table, Row, Cell};

use serde::{Serialize, Deserialize};
use crate::server::options::{Init, StartConf};
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::detect::{self, VersionQuery, InstalledPackage};
use crate::table;


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
    let settings = Settings {
        name: options.name.clone(),
        system: options.system,
        version,
        method: meth_name,
        directory: data_path(options.system)?.join(&options.name),
        port: options.port,
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
