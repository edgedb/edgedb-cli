use std::fs;
use std::process::Command;
use std::path::{Path, PathBuf};
use std::collections::HashSet;

use anyhow::Context;
use prettytable::{Table, Row, Cell};

use serde::{Serialize, Deserialize};
use crate::server::options::Init;
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::server::methods::InstallMethod;
use crate::server::detect::{self, VersionQuery};
use crate::table;


pub struct Settings {
    pub name: String,
    pub system: bool,
    pub version: Version<String>,
    pub method: InstallMethod,
    pub directory: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Metadata {
    version: Version<String>,
    method: InstallMethod,
}

fn data_path(system: bool) -> anyhow::Result<PathBuf> {
    if system {
        todo!();
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

pub fn init(options: &Init) -> anyhow::Result<()> {
    let version_query = VersionQuery::new(
        options.nightly, &options.version);
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (version, meth_name, method) = if let Some(ref _meth) = options.method
    {
        todo!();
    } else if version_query.is_nightly() || version_query.is_specific() {
        todo!();
    } else {
        let methods = avail_methods.instantiate_all(&*current_os, true)?;
        let mut max_ver = None;
        let mut ver_methods = HashSet::new();
        for (meth, method) in &methods {
            for ver in method.installed_versions()? {
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
        if let Some(ver) = max_ver {
            let mut ver_methods = ver_methods.into_iter().collect::<Vec<_>>();
            ver_methods.sort();
            let mut methods = methods;
            let meth_name = ver_methods.remove(0);
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
    };
    settings.print();
    if settings.system {
        todo!();
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
                You can run service manually via: \n  \
                edgedb server start --foreground");
            e
        }).context("failed to init service")?;
        println!("Bootstrap complete. To start a server:\n  \
                  edgedb server start");
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
