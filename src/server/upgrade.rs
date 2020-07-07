use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;

use anyhow::Context;
use linked_hash_map::LinkedHashMap;

use crate::server::control::get_instance_from_metadata;
use crate::server::detect::{self, VersionQuery};
use crate::server::init::{Metadata, data_path};
use crate::server::install;
use crate::server::options::{self, Upgrade};
use crate::server::os_trait::Method;
use crate::server::version::Version;


enum ToDo {
    MinorUpgrade,
    InstanceUpgrade(String, VersionQuery),
    NightlyUpgrade,
}

struct InstanceIterator {
    dir: fs::ReadDir,
    path: PathBuf,
}

fn interpret_options(options: &Upgrade) -> ToDo {
    if let Some(name) = &options.name {
        if options.nightly {
            eprintln!("Cannot upgrade specific nightly instance, \
                use `--to-nightly` to upgrade to nightly. \
                Use `--nightly` without instance name to upgrade all nightly \
                instances");
        }
        let nver = if options.to_nightly {
            VersionQuery::Nightly
        } else if let Some(ver) = &options.to_version {
            VersionQuery::Stable(Some(ver.clone()))
        } else {
            VersionQuery::Stable(None)
        };
        ToDo::InstanceUpgrade(name.into(), nver)
    } else if options.nightly {
        ToDo::NightlyUpgrade
    } else {
        ToDo::MinorUpgrade
    }
}

fn all_instances() -> anyhow::Result<Vec<(String, Metadata)>> {
    let path = data_path(false)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    InstanceIterator {
        dir: fs::read_dir(&path)?,
        path: path.into(),
    }.collect::<Result<Vec<_>,_>>()
}

fn read_metadata(path: &Path) -> anyhow::Result<Metadata> {
    let file = fs::read(path)
        .with_context(|| format!("error reading {}", path.display()))?;
    let metadata = serde_json::from_slice(&file)
        .with_context(|| format!("error decoding json {}", path.display()))?;
    Ok(metadata)
}

impl Iterator for InstanceIterator {
    type Item = anyhow::Result<(String, Metadata)>;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.dir.next() {
            match self.read_item(item).transpose() {
                None => continue,
                val => return val,
            }
        }
        return None;
    }
}

impl InstanceIterator {
    fn read_item(&self, item: Result<fs::DirEntry, io::Error>)
        -> anyhow::Result<Option<(String, Metadata)>>
    {
        let item = item.with_context(
            || format!("error listing instances dir {}",
                       self.path.display()))?;
        if !item.file_type()
            .with_context(|| format!(
                "error listing {}: cannot determine entry type",
                self.path.display()))?
            .is_dir()
        {
            return Ok(None);
        }
        if let Some(name) = item.file_name().to_str() {
            if name.starts_with(".") {
                return Ok(None);
            }
            let metadata = match
                read_metadata(&item.path().join("metadata.json"))
            {
                Ok(metadata) => metadata,
                Err(e) => {
                    log::warn!(target: "edgedb::server::upgrade",
                        "Error reading metadata for \
                        instance {:?}: {:#}. Skipping...",
                        name, e);
                    return Ok(None);
                }
            };
            return Ok(Some((name.into(), metadata)));
        } else {
            return Ok(None);
        }
    }
}

fn get_instances(todo: &ToDo) -> anyhow::Result<BTreeMap<String, Metadata>> {
    use ToDo::*;

    let instances = match todo {
        MinorUpgrade => all_instances()?.into_iter()
            .filter(|(_, meta)| !meta.version.as_ref().contains("nightly"))
            .collect(),
        NightlyUpgrade => all_instances()?.into_iter()
            .filter(|(_, meta)| meta.version.as_ref().contains("nightly"))
            .collect(),
        InstanceUpgrade(name, ..) => all_instances()?.into_iter()
            .filter(|(n, _)| n == name)
            .collect(),
    };
    Ok(instances)
}

pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    use ToDo::*;

    let todo = interpret_options(&options);
    let instances = get_instances(&todo)?;
    if instances.is_empty() {
        log::warn!(target: "edgedb::server::upgrade",
            "No instances found. Nothing to upgrade.");
        return Ok(());
    }
    let mut by_method = BTreeMap::new();
    for (name, meta) in instances {
        by_method.entry(meta.method.clone())
            .or_insert_with(BTreeMap::new)
            .insert(name, meta);
    }

    let os = detect::current_os()?;
    let avail = os.get_available_methods()?;
    for (meth_name, instances) in by_method {
        if !avail.is_supported(&meth_name) {
            log::warn!(target: "edgedb::server::upgrade",
                "method {} is not available. \
                Instances using it {}. Skipping",
                meth_name.title(),
                instances
                    .iter()
                    .map(|(n, _)| &n[..])
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            continue;
        }
        let method = os.make_method(&meth_name, &avail)?;
        match todo {
            MinorUpgrade => {
                do_minor_upgrade(&*method, instances)?;
            }
            NightlyUpgrade => {
                do_nightly_upgrade(&*method, instances)?;
            }
            InstanceUpgrade(.., ref version) => {
                for (name, meta) in instances {
                    do_instance_upgrade(&*method, name, meta, version)?;
                }
            }
        }
    }
    Ok(())
}

fn do_minor_upgrade(method: &dyn Method,
    instances: BTreeMap<String, Metadata>)
    -> anyhow::Result<()>
{
    let mut by_major = BTreeMap::new();
    for (name, meta) in instances {
        by_major.entry(meta.version.clone())
            .or_insert_with(BTreeMap::new)
            .insert(name, meta);
    }
    for (version, instances) in by_major {
        let instances_str = instances
            .iter().map(|(n, _)| &n[..]).collect::<Vec<_>>().join(", ");

        let version_query = VersionQuery::Stable(Some(version.clone()));
        let new = method.get_version(&version_query)
            .context("Unable to determine version")?;

        let new_ver = Version(format!("{}-{}", new.version, new.revision));
        for ver in method.installed_versions()? {
            if ver.major_version != version {
                continue
            }
            let cur_ver = Version(format!("{}-{}", new.version, new.revision));
            if cur_ver <= new_ver {
                log::info!(target: "edgedb::server::upgrade",
                    "Version {} is up to date {}, skipping instances: {}",
                    version, cur_ver, instances_str);
                return Ok(());
            }
        }

        println!("Upgrading version: {} to {}-{}, instances: {}",
            version, new.version, new.revision, instances_str);

        // Stop instances first.
        //
        // This (launchctl unload) is required for MacOS to reinstall
        // the pacakge. On other systems, this is also useful as in-place
        // modifying the running package isn't very good idea.
        for (name, meta) in &instances {
            let mut instance = get_instance_from_metadata(name, meta, false)?;
            instance.stop(&options::Stop { name: name.clone() })
                .map_err(|e| {
                    log::warn!("Failed to stop instance {:?}: {:#}", name, e);
                })
                .ok();
        }


        method.install(&install::Settings {
            method: method.name(),
            package_name: new.package_name,
            major_version: version,
            version: new.version,
            nightly: false,
            extra: LinkedHashMap::new(),
        })?;

        for (name, meta) in &instances {
            let mut instance = get_instance_from_metadata(name, meta, false)?;
            instance.start(&options::Start { name: name.clone() })?;
        }
    }
    Ok(())
}

fn do_nightly_upgrade(method: &dyn Method,
    instances: BTreeMap<String, Metadata>)
    -> anyhow::Result<()>
{
    // START IF NOT RUNNING
    // DUMP
    // STOP
    // REINSTALL
    // START
    // RESTORE
    todo!();
}

fn do_instance_upgrade(method: &dyn Method,
    name: String, meta: Metadata, version: &VersionQuery)
    -> anyhow::Result<()>
{
    // INSTALL
    // DUMP
    // STOP
    // MODIFY SERVICE FILE
    // START
    // RESTORE
    todo!();
}
