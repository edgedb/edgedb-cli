use std::collections::{BTreeSet, BTreeMap};

use prettytable::{Table, Cell, Row};

use crate::server::detect;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::ListVersions;
use crate::server::version::Version;
use crate::server::distribution::{MajorVersion, DistributionRef};
use crate::table;


#[derive(Debug)]
pub struct VersionInfo {
    available: BTreeSet<InstallMethod>,
    installed: BTreeMap<InstallMethod, DistributionRef>,
    latest: Version<String>,
}


pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    let mut versions = BTreeMap::new();
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    if options.installed_only {
        remote(&methods, &mut versions)
            .map_err(|e| {
                log::warn!("Error fetching remote versions: {:#}", e);
            }).ok();
        installed(&methods, &mut versions)?;
        let versions = versions.into_iter()
            .filter(|(_m, v)| !v.installed.is_empty())
            .collect();
        print_versions(versions);
    } else {
        remote(&methods, &mut versions)?;
        installed(&methods, &mut versions)
            .map_err(|e| {
                log::warn!("Error fetching installed versions: {:#}", e);
            }).ok();
        print_versions(versions);
    }
    Ok(())
}

fn installed(methods: &Methods,
    versions: &mut BTreeMap<MajorVersion, VersionInfo>)
    -> Result<(), anyhow::Error>
{
    for (meth, method) in methods {
        for distr in method.installed_versions()? {
            let entry = versions.entry(distr.major_version().clone())
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

fn remote(methods: &Methods,
    versions: &mut BTreeMap<MajorVersion, VersionInfo>)
    -> anyhow::Result<()>
{
    for (meth, method) in methods {
        let nightly = method.all_versions(true)?;
        let stable = method.all_versions(false)?;
        for distr in stable.iter().chain(nightly.iter()) {
            let info = versions.entry(distr.major_version().clone())
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

fn print_versions(versions: BTreeMap<MajorVersion, VersionInfo>) {
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
            Cell::new(info.latest.as_ref())
                .style_spec(if info.installed.is_empty() {
                    ""
                } else if info.installed.iter()
                          .all(|(_m, distr)| distr.version() == &info.latest)
                {
                    "bFg"
                } else {
                    "bFr"
                }),
            Cell::new(&info.available.iter()
                .map(|x| x.short_name())
                .collect::<Vec<_>>()
                .join(", ")),
            Cell::new(&info.installed.iter()
                .map(|(meth, distr)| {
                    if distr.version() == &info.latest {
                        meth.short_name().to_owned()
                    } else {
                        format!("{}:{}", meth.short_name(), distr.version())
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")),
            Cell::new(&ver.option()),
        ]));
    }
    table.printstd();
}
