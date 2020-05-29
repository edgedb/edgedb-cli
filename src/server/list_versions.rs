use std::collections::{BTreeSet, BTreeMap};

use prettytable::{Table, Cell, Row};

use crate::server::detect;
use crate::server::methods::{InstallMethod, Methods};
use crate::server::options::ListVersions;
use crate::server::version::Version;
use crate::table;


#[derive(Debug)]
pub struct VersionInfo {
    available: BTreeSet<InstallMethod>,
    installed: BTreeMap<InstallMethod, Version<String>>,
    full: Version<String>,
    nightly: bool,
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
    versions: &mut BTreeMap<Version<String>, VersionInfo>)
    -> Result<(), anyhow::Error>
{
    for (meth, method) in methods {
        for ver in method.installed_versions()? {
            let full_ver = format!("{}-{}", ver.version, ver.revision);
            let entry = versions.entry(ver.major_version.clone())
                .or_insert_with(|| VersionInfo {
                    available: BTreeSet::new(),
                    installed: BTreeMap::new(),
                    full: Version(full_ver.clone()),
                    nightly: false,
                });
            entry.installed.insert(meth.clone(), Version(full_ver));
        }
    }
    Ok(())
}

fn remote(methods: &Methods, versions: &mut BTreeMap<Version<String>, VersionInfo>)
    -> anyhow::Result<()>
{
    for (meth, method) in methods {
        for pkg in method.all_versions(false)? {
            if let Some(major) = &pkg.slot {
                let full_ver = Version(format!("{}-{}",
                    pkg.version, pkg.revision));
                let ver = versions.entry(major.clone())
                    .or_insert_with(|| VersionInfo {
                        available: BTreeSet::new(),
                        installed: BTreeMap::new(),
                        full: pkg.version.clone(),
                        nightly: false,
                    });
                ver.available.insert(meth.clone());
                if ver.full < full_ver {
                    ver.full = full_ver;
                }
            }
        }
        for pkg in method.all_versions(true)? {
            if let Some(major) = &pkg.slot {
                let ver = versions.entry(major.clone())
                    .or_insert_with(|| VersionInfo {
                        available: BTreeSet::new(),
                        installed: BTreeMap::new(),
                        full: pkg.version.clone(),
                        nightly: true,
                    });
                if ver.nightly {
                    ver.available.insert(meth.clone());
                    if ver.full < pkg.version {
                        ver.full = pkg.version.clone();
                    }
                }
            }
        }
    }
    Ok(())
}

fn print_versions(versions: BTreeMap<Version<String>, VersionInfo>) {
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
        let ver_option = format!("--version={}", ver);
        let nightly_ver = format!("{} (nightly)", ver);
        table.add_row(Row::new(vec![
            Cell::new(if info.nightly {
                    &nightly_ver
                } else {
                    ver.as_ref()
                }),
            Cell::new(info.full.as_ref())
                .style_spec(if info.installed.is_empty() {
                    ""
                } else if info.installed.iter()
                          .all(|(_m, ver)| ver == &info.full)
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
                    if ver == &info.full {
                        meth.short_name().to_owned()
                    } else {
                        format!("{}:{}", meth.short_name(), ver)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")),
            Cell::new(if info.nightly {
                    "--nightly"
                } else {
                    &ver_option
                }),
        ]));
    }
    table.printstd();
}
