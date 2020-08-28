use std::collections::{BTreeSet, BTreeMap};

use prettytable::{Table, Cell, Row};

use crate::server::detect;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::ListVersions;
use crate::server::version::Version;
use crate::server::distribution::MajorVersion;
use crate::server::os_trait::{PreciseVersion};
use crate::table;


#[derive(Debug)]
pub struct VersionInfo {
    available: BTreeSet<InstallMethod>,
    installed: BTreeMap<InstallMethod, Version<String>>,
    precise: PreciseVersion,
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
    /* TODO
    for (meth, method) in methods {
        for ver in method.installed_versions()? {
            let full_ver = format!("{}-{}", ver.version, ver.revision);
            let entry = versions.entry(ver.major_version.clone())
                .or_insert_with(|| VersionInfo {
                    available: BTreeSet::new(),
                    installed: BTreeMap::new(),
                    full: Version(full_ver.clone()),
                });
            entry.installed.insert(meth.clone(), Version(full_ver));
        }
    }
    */
    Ok(())
}

fn remote(methods: &Methods,
    versions: &mut BTreeMap<MajorVersion, VersionInfo>)
    -> anyhow::Result<()>
{
    for (meth, method) in methods {
        let nightly = method.all_versions(true)?;
        let stable = method.all_versions(false)?;
        for ver in stable.iter().chain(nightly.iter()) {
            let info = versions.entry(ver.major().clone())
                .or_insert_with(|| VersionInfo {
                    available: BTreeSet::new(),
                    installed: BTreeMap::new(),
                    precise: ver.clone(),
                });
            info.available.insert(meth.clone());
            if &info.precise < ver {
                info.precise = ver.clone();
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
            Cell::new(info.precise.as_str())
                .style_spec(if info.installed.is_empty() {
                    ""
                } else if info.installed.iter()
                          .all(|(_m, ver)| ver == info.precise.as_ver())
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
                .map(|(meth, ver)| {
                    if ver == info.precise.as_ver() {
                        meth.short_name().to_owned()
                    } else {
                        format!("{}:{}", meth.short_name(), ver)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")),
            Cell::new(&ver.option()),
        ]));
    }
    table.printstd();
}
