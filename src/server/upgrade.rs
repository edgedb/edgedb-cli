use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Context;
use async_std::task;
use linked_hash_map::LinkedHashMap;
use fn_error_context::context;

use crate::client;
use crate::server::control::get_instance_from_metadata;
use crate::server::detect::{self, VersionQuery, VersionResult};
use crate::server::init::{init, Metadata, data_path};
use crate::server::install;
use crate::server::options::{self, Upgrade};
use crate::server::os_trait::Method;
use crate::server::version::Version;
use crate::commands;
use crate::platform::ProcessGuard;


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

fn is_ident(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for c in chars {
        if !c.is_alphanumeric() && c != '_' {
            return false;
        }
    }
    return true
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
            if !is_ident(name) {
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
            .filter(|(_, meta)| !meta.nightly)
            .collect(),
        NightlyUpgrade => all_instances()?.into_iter()
            .filter(|(_, meta)| meta.nightly)
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
        if options.nightly {
            log::warn!(target: "edgedb::server::upgrade",
                "No instances found. Nothing to upgrade.");
        } else {
            log::warn!(target: "edgedb::server::upgrade",
                "No instances found. Nothing to upgrade \
                (Note: nightly instances are upgraded only if `--nightly` \
                is specified).");
        }
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
                Instances using it {}. Skipping...",
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
                do_minor_upgrade(&*method, instances, options)?;
            }
            NightlyUpgrade => {
                do_nightly_upgrade(&*method, instances, options)?;
            }
            InstanceUpgrade(.., ref version) => {
                for (name, meta) in instances {
                    do_instance_upgrade(&*method,
                        name, meta, version, options)?;
                }
            }
        }
    }
    Ok(())
}

fn do_minor_upgrade(method: &dyn Method,
    instances: BTreeMap<String, Metadata>, options: &Upgrade)
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

        if !options.force {
            if let Some(cur_ver) = up_to_date(&version_query, &new, method)? {
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

        log::info!(target: "edgedb::server::upgrade", "Upgrading the package");
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
            instance.start(&options::Start {
                name: name.clone(),
                foreground: false,
            })?;
        }
    }
    Ok(())
}

async fn dump_instance(name: &str, _meta: &Metadata, socket: &Path)
    -> anyhow::Result<()>
{
    log::info!(target: "edgedb::server::upgrade",
        "Dumping instance {:?}", name);
    let path = data_path(false)?.join(format!("{}.dump", name));
    if path.exists() {
        log::info!(target: "edgedb::server::upgrade",
            "Removing old dump at {}", path.display());
        fs::remove_dir_all(&path)?;
    }
    let mut conn_params = client::Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(socket);
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::dump_all(&mut cli, &options, path.as_ref()).await?;
    Ok(())
}

async fn restore_instance(name: &str, _meta: &Metadata, socket: &Path)
    -> anyhow::Result<()>
{
    use crate::commands::parser::Restore;

    log::info!(target: "edgedb::server::upgrade",
        "Restoring instance {:?}", name);
    let path = data_path(false)?.join(format!("{}.dump", name));
    let mut conn_params = client::Builder::new();
    conn_params.user("edgedb");
    conn_params.database("edgedb");
    conn_params.unix_addr(socket);
    conn_params.wait_until_available(Duration::from_secs(30));
    let mut cli = conn_params.connect().await?;
    let options = commands::Options {
        command_line: true,
        styler: None,
        conn_params,
    };
    commands::restore_all(&mut cli, &options, &Restore {
        path,
        all: true,
        allow_non_empty: false,
        verbose: false,
    }).await?;
    Ok(())
}

fn do_nightly_upgrade(method: &dyn Method,
    instances: BTreeMap<String, Metadata>, options: &Upgrade)
    -> anyhow::Result<()>
{
    let instances_str = instances
        .iter().map(|(n, _)| &n[..]).collect::<Vec<_>>().join(", ");

    let version_query = VersionQuery::Nightly;
    let new = method.get_version(&version_query)
        .context("Unable to determine version")?;

    if !options.force {
        if let Some(cur_ver) = up_to_date(&version_query, &new, method)? {
            log::info!(target: "edgedb::server::upgrade",
                "Nightly is up to date {}, skipping instances: {}",
                cur_ver, instances_str);
            return Ok(());
        }
    }

    for (name, meta) in &instances {
        dump_and_stop(name, meta, false)?;
    }

    log::info!(target: "edgedb::server::upgrade", "Upgrading the package");
    method.install(&install::Settings {
        method: method.name(),
        package_name: new.package_name,
        major_version: new.major_version,
        version: new.version,
        nightly: true,
        extra: LinkedHashMap::new(),
    })?;

    for (name, meta) in &instances {
        reinit_and_restore(name, meta, false, method)?;
    }
    Ok(())
}

#[context("failed to dump {:?}", name)]
fn dump_and_stop(name: &str, meta: &Metadata, system: bool)
    -> anyhow::Result<()>
{
    let mut inst = get_instance_from_metadata(name, meta, system)?;
    // in case not started for now
    log::info!(target: "edgedb::server::upgrade",
        "Ensuring instance is started");
    inst.start(&options::Start { name: name.into(), foreground: false })?;
    task::block_on(dump_instance(name, meta, &inst.get_socket(true)?))?;
    log::info!(target: "edgedb::server::upgrade",
        "Stopping the instance before package upgrade");
    inst.stop(&options::Stop { name: name.into() })?;
    Ok(())
}

#[context("failed to restore {:?}", name)]
fn reinit_and_restore(name: &str, meta: &Metadata, system: bool,
    method: &dyn Method)
    -> anyhow::Result<()>
{
    let base = data_path(false)?;
    let path = base.join(&name);
    let backup = base.join(&format!("{}.backup", name));
    fs::rename(path, backup)?;
    init(&options::Init {
        name: name.into(),
        system,
        interactive: false,
        nightly: true,
        version: None,
        method: Some(method.name()),
        port: Some(meta.port),
        start_conf: meta.start_conf,
        inhibit_user_creation: true,
        inhibit_start: true,
    })?;

    let mut inst = get_instance_from_metadata(name, meta, system)?;
    let mut cmd = inst.run_command()?;
    // temporarily patch the edgedb issue of 1-alpha.4
    cmd.arg("--default-database=edgedb");
    cmd.arg("--default-database-user=edgedb");
    log::debug!("Running server: {:?}", cmd);
    let child = ProcessGuard::run(&mut cmd)
        .with_context(|| format!("error running server {:?}", cmd))?;

    task::block_on(restore_instance(name, meta,
                                    &inst.get_socket(true)?))?;
    log::info!(target: "edgedb::server::upgrade",
        "Restarting instance {:?} to apply changes from `restore --all`",
        name);
    drop(child);

    inst.start(&options::Start { name: name.into(), foreground: false })?;
    Ok(())
}

fn up_to_date(version: &VersionQuery, new: &VersionResult, method: &dyn Method)
    -> anyhow::Result<Option<Version<String>>>
{
    let new_ver = Version(format!("{}-{}", new.version, new.revision));
    for ver in method.installed_versions()? {
        if !version.installed_matches(ver) {
            continue
        }
        let cur_ver = Version(format!("{}-{}",
                                      ver.version, ver.revision));
        if cur_ver >= new_ver {
            return Ok(Some(cur_ver));
        }
    }
    return Ok(None);
}

fn do_instance_upgrade(method: &dyn Method,
    name: String, meta: Metadata, version: &VersionQuery, options: &Upgrade)
    -> anyhow::Result<()>
{
    let new = method.get_version(&version)
        .context("Unable to determine version")?;

    if !options.force {
        if let Some(cur_ver) = up_to_date(version, &new, method)? {
            log::info!(target: "edgedb::server::upgrade",
                "Version {} is up to date {}, skipping instance: {}",
                version, cur_ver, name);
            return Ok(());
        }
    }

    dump_and_stop(&name, &meta, false)?;

    log::info!(target: "edgedb::server::upgrade", "Installing the package");
    method.install(&install::Settings {
        method: method.name(),
        package_name: new.package_name,
        major_version: new.major_version,
        version: new.version,
        nightly: version.is_nightly(),
        extra: LinkedHashMap::new(),
    })?;

    reinit_and_restore(&name, &meta, false, method)?;
    Ok(())
}
