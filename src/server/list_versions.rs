use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use linked_hash_map::LinkedHashMap;
use prettytable::{Cell, Row, Table};

use crate::server::detect;
use crate::server::distribution::{DistributionRef, MajorVersion};
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::ListVersions;
use crate::server::version::Version;
use crate::table;

#[derive(Debug)]
pub struct VersionInfo {
    available: BTreeSet<InstallMethod>,
    installed: BTreeMap<InstallMethod, DistributionRef>,
    latest: Version<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct JsonVersionInfo<'a> {
    major_version: &'a MajorVersion,
    latest_version: &'a Version<String>,
    available_for_methods: Vec<&'a str>,
    installed: LinkedHashMap<&'a str, &'a Version<String>>,
    option_to_install: String,
}

pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    let mut versions = BTreeMap::new();
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    if options.installed_only {
        remote(&methods, &mut versions)
            .map_err(|e| {
                log::warn!("Error fetching remote versions: {:#}", e);
            })
            .ok();
        installed(&methods, &mut versions)?;
        let versions: BTreeMap<_, _> = versions
            .into_iter()
            .filter(|(_m, v)| !v.installed.is_empty())
            .collect();
        print_versions(versions, options)?;
    } else {
        remote(&methods, &mut versions)?;
        installed(&methods, &mut versions)
            .map_err(|e| {
                log::warn!("Error fetching installed versions: {:#}", e);
            })
            .ok();
        print_versions(versions, options)?;
    }
    Ok(())
}

fn installed(
    methods: &Methods,
    versions: &mut BTreeMap<MajorVersion, VersionInfo>,
) -> Result<(), anyhow::Error> {
    for (meth, method) in methods {
        for distr in method.installed_versions()? {
            let entry = versions
                .entry(distr.major_version().clone())
                .or_insert_with(|| VersionInfo {
                    available: BTreeSet::new(),
                    installed: BTreeMap::new(),
                    latest: distr.version().clone(),
                });
            entry.installed.insert(meth.clone(), distr);
        }
    }
    Ok(())
}

fn remote(
    methods: &Methods,
    versions: &mut BTreeMap<MajorVersion, VersionInfo>,
) -> anyhow::Result<()> {
    for (meth, method) in methods {
        let nightly = method.all_versions(true)?;
        let stable = method.all_versions(false)?;
        for distr in stable.iter().chain(nightly.iter()) {
            let info = versions
                .entry(distr.major_version().clone())
                .or_insert_with(|| VersionInfo {
                    available: BTreeSet::new(),
                    installed: BTreeMap::new(),
                    latest: distr.version().clone(),
                });
            info.available.insert(meth.clone());
            if &info.latest < distr.version() {
                info.latest = distr.version().clone();
            }
        }
    }
    Ok(())
}

fn print_set<V: fmt::Display>(vals: impl IntoIterator<Item = V>, json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &vals.into_iter().map(|v| v.to_string()).collect::<Vec<_>>()
            )?
        );
    } else {
        for item in vals {
            println!("{}", item);
        }
    }
    Ok(())
}

fn print_versions(
    versions: BTreeMap<MajorVersion, VersionInfo>,
    options: &ListVersions,
) -> anyhow::Result<()> {
    match options.column.as_ref().map(|s| &s[..]) {
        None if options.json => print_json(versions),
        None => print_table(versions),
        Some("major-version") => print_set(versions.keys().map(|v| v.title()), options.json),
        Some("available") => print_set(versions.values().map(|info| &info.latest), options.json),
        Some("installed") => print_set(
            versions
                .values()
                .flat_map(|info| info.installed.values())
                .map(|v| v.version())
                .collect::<BTreeSet<_>>(),
            options.json,
        ),
        Some(col) => {
            anyhow::bail!("unexpected --column={:?}", col);
        }
    }
}

fn print_json(versions: BTreeMap<MajorVersion, VersionInfo>) -> anyhow::Result<()> {
    print!(
        "{}",
        serde_json::to_string_pretty(
            &versions
                .iter()
                .map(|(ver, info)| JsonVersionInfo {
                    major_version: ver,
                    latest_version: &info.latest,
                    available_for_methods: info
                        .available
                        .iter()
                        .map(|x| x.short_name())
                        .collect::<Vec<_>>(),
                    installed: info
                        .installed
                        .iter()
                        .map(|(meth, distr)| (meth.short_name(), distr.version()))
                        .collect::<LinkedHashMap<_, _>>(),
                    option_to_install: ver.option(),
                })
                .collect::<Vec<_>>()
        )?
    );
    Ok(())
}

fn print_table(versions: BTreeMap<MajorVersion, VersionInfo>) -> anyhow::Result<()> {
    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.add_row(Row::new(vec![
        table::header_cell("Major Version"),
        table::header_cell("Full Version"),
        table::header_cell("Available"),
        table::header_cell("Installed"),
        table::header_cell("Param"),
    ]));
    for (ver, info) in &versions {
        table.add_row(Row::new(vec![
            Cell::new(ver.title()),
            Cell::new(info.latest.as_ref()).style_spec(if info.installed.is_empty() {
                ""
            } else if info
                .installed
                .iter()
                .all(|(_m, distr)| distr.version() == &info.latest)
            {
                "bFg"
            } else {
                "bFr"
            }),
            Cell::new(
                &info
                    .available
                    .iter()
                    .map(|x| x.short_name())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Cell::new(
                &info
                    .installed
                    .iter()
                    .map(|(meth, distr)| {
                        if distr.version() == &info.latest {
                            meth.short_name().to_owned()
                        } else {
                            format!("{}:{}", meth.short_name(), distr.version())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Cell::new(&ver.option()),
        ]));
    }
    table.printstd();
    Ok(())
}
