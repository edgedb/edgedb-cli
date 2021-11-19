use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::server::options::ListVersions;

use crate::echo;
use crate::portable::local::{self, InstallInfo};
use crate::portable::repository::{get_server_packages, Channel, PackageInfo};
use crate::portable::ver;
use crate::print::Highlight;
use crate::table::{self, Table, Row, Cell};


#[derive(serde::Serialize)]
pub struct DebugInstall {
    path: Option<PathBuf>,
    server_path: Option<PathBuf>,
    #[serde(flatten)]
    install: InstallInfo,
}

#[derive(serde::Serialize)]
pub struct DebugInfo {
    #[serde(skip_serializing_if="Option::is_none")]
    package: Option<PackageInfo>,
    #[serde(skip_serializing_if="Option::is_none")]
    install: Option<DebugInstall>,
}

pub struct Pair {
    package: Option<PackageInfo>,
    install: Option<InstallInfo>,
}


#[derive(serde::Serialize)]
#[serde(rename_all="kebab-case")]
pub struct JsonVersionInfo {
    channel: Channel,
    version: ver::Build,
    installed: bool,
    debug_info: DebugInfo,
}

pub fn all_packages() -> Vec<PackageInfo> {
    let mut pkgs = Vec::with_capacity(16);
    match get_server_packages(Channel::Stable) {
        Ok(stable) => pkgs.extend(stable),
        Err(e) => log::warn!("Unable to fetch stable packages: {:#}", e),
    };
    match get_server_packages(Channel::Nightly) {
        Ok(nightly) => pkgs.extend(nightly),
        Err(e) => log::warn!("Unable to fetch nightly packages: {:#}", e),
    }
    return pkgs;
}

pub fn list_versions(options: &ListVersions) -> Result<(), anyhow::Error> {
    if options.deprecated_install_methods {
        return crate::server::list_versions::list_versions(options);
    }
    let mut installed = local::get_installed()?;
    if options.installed_only {
        if options.json {
            print!("{}", serde_json::to_string_pretty(
                &installed.into_iter()
                    .map(|v| JsonVersionInfo {
                        channel: Channel::from_version(&v.version.specific())
                            .unwrap_or(Channel::Nightly),
                        version: v.version.clone(),
                        installed: true,
                        debug_info: DebugInfo {
                            install: Some(DebugInstall::from(v)),
                            package: None,
                        },
                    })
                    .collect::<Vec<_>>()
            )?);
        } else {
            installed.sort_by(|a, b| a.version.specific()
                              .cmp(&b.version.specific()));
            print_table(installed.into_iter().map(|p| (p.version, true)));
        }
    } else {
        let mut version_set = BTreeMap::new();
        for package in all_packages() {
            version_set.insert(package.version.specific(), Pair {
                package: Some(package),
                install: None,
            });
        }
        for install in installed {
            let _ = version_set.entry(install.version.specific())
                .or_insert_with(|| Pair { package: None, install: None })
                .install.insert(install);
        }
        if options.json {
            print!("{}", serde_json::to_string_pretty(
                &version_set.into_iter()
                    .map(|(ver, vp)| JsonVersionInfo {
                        channel: Channel::from_version(&ver)
                            .unwrap_or(Channel::Nightly),
                        version: vp.install.as_ref().map_or_else(
                            || vp.package.as_ref().unwrap().version.clone(),
                            |v| v.version.clone(),
                        ),
                        installed: vp.install.is_some(),
                        debug_info: DebugInfo {
                            install: vp.install.map(DebugInstall::from),
                            package: vp.package,
                        },
                    })
                    .collect::<Vec<_>>()
            )?);
        } else {
            print_table(version_set.into_iter()
                        .map(|(_, vp)| match vp.install {
                            Some(v) => (v.version, true),
                            None => (vp.package.unwrap().version, false),
                        }));
        }
    }
    echo!("Only portable packages shown here, \
        use `--deprecated-install-methods` \
        to show docker and package installations.".fade());
    Ok(())
}

fn print_table(items: impl Iterator<Item=(ver::Build, bool)>) {
    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.add_row(Row::new(vec![
        table::header_cell("Channel"),
        table::header_cell("Version"),
        table::header_cell("Installed"),
    ]));
    for (ver, installed) in items {
        let channel = Channel::from_version(&ver.specific());
        table.add_row(Row::new(vec![
            Cell::new(&channel.as_ref().map_or("nightly", |x| x.as_str())),
            Cell::new(&ver.to_string()),
            Cell::new(if installed { "✓" } else { "" }),
        ]));
    }
    table.printstd();
}

impl DebugInstall {
    fn from(install: InstallInfo) -> DebugInstall {
        DebugInstall {
            path: install.base_path().ok(),
            server_path: install.server_path().ok(),
            install,
        }
    }
}
