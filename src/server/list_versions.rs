use std::collections::{BTreeSet, BTreeMap};

use prettytable::{Table, Cell, Row};

use crate::server::detect;
use crate::server::install::InstallMethod;
use crate::server::options::ListVersions;
use crate::server::version::Version;
use crate::table;


#[derive(Debug)]
pub struct VersionInfo {
    available: BTreeSet<InstallMethod>,
    installed: BTreeSet<InstallMethod>,
    full: Version<String>,
    nightly: bool,
}


pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    if options.installed_only {
        installed()
    } else {
        all()
    }
}

fn installed() -> Result<(), anyhow::Error> {
    todo!()
}

fn all() -> Result<(), anyhow::Error> {
    let os = detect::current_os()?;
    //let mut installed = Vec::new();
    let mut versions = BTreeMap::new();
    for (meth, method) in os.instantiate_methods()? {
        for pkg in method.all_versions(false)? {
            if let Some(major) = &pkg.slot {
                let full_ver = Version(format!("{}-{}",
                    pkg.version, pkg.revision));
                let ver = versions.entry(major.clone())
                    .or_insert_with(|| VersionInfo {
                        available: BTreeSet::new(),
                        installed: BTreeSet::new(),
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
                        installed: BTreeSet::new(),
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
            Cell::new(info.full.as_ref()),
            Cell::new(&info.available.iter()
                .map(|x| x.short_name())
                .collect::<Vec<_>>()
                .join(", ")),
            Cell::new(&info.installed.iter()
                .map(|x| x.short_name())
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
    Ok(())
}
