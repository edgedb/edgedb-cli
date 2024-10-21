use libc::option;
use prettytable::{row, Table};

use crate::hint::HintExt;
use crate::{platform, table};
use crate::portable::local::InstanceInfo;
use crate::portable::options::{instance_arg, InstanceName, ServerInstanceExtensionCommand};
use crate::options::Options;
use crate::portable::platform::get_server;
use crate::portable::repository::{get_platform_extension_packages, Channel};
use super::options::{ExtensionInstall, ExtensionList, ExtensionListExtensions};

pub fn extension_main(c: &ServerInstanceExtensionCommand, options: &Options) -> Result<(), anyhow::Error> {
    use crate::portable::options::InstanceExtensionCommand::*;
    match &c.subcommand {
        Install(c) => install(c),
        List(c) => list(c),
        ListAvailable(c) => list_extensions(c),
    }
}

fn list(options: &ExtensionList) -> Result<(), anyhow::Error> {
    let name = match instance_arg(&None, &options.instance)? {
        InstanceName::Local(name) => name,
        inst_name => {
            return Err(anyhow::anyhow!(
                "cannot install extensions in cloud instance {}.",
                inst_name
            ))
            .with_hint(|| {
                format!(
                    "only local instances can install extensions ({} is remote)",
                    inst_name
                )
            })?;
        }
    };
    let Some(inst) = InstanceInfo::try_read(&name)? else {
        return Err(anyhow::anyhow!(
            "cannot install extensions in cloud instance {}.",
            name
        ))
        .with_hint(|| {
            format!(
                "only local instances can install extensions ({} is remote)",
                name
            )
        })?;
    };
    eprintln!("{:?}", inst.extension_path()?);
    for file in inst.extension_path()?.read_dir()? {
        let file = file?;
        if file.metadata()?.is_dir() {
            eprintln!(" - {:?}", file.file_name());
        }
    }
    Ok(())
}

fn install(options: &ExtensionInstall) -> Result<(), anyhow::Error> {
    let name = match instance_arg(&None, &options.instance)? {
        InstanceName::Local(name) => name,
        inst_name => {
            return Err(anyhow::anyhow!(
                "cannot install extensions in cloud instance {}.",
                inst_name
            ))
            .with_hint(|| {
                format!(
                    "only local instances can install extensions ({} is remote)",
                    inst_name
                )
            })?;
        }
    };
    let Some(inst) = InstanceInfo::try_read(&name)? else {
        return Err(anyhow::anyhow!(
            "cannot install extensions in cloud instance {}.",
            name
        ))
        .with_hint(|| {
            format!(
                "only local instances can install extensions ({} is remote)",
                name
            )
        })?;
    };

    let version = inst.get_version()?.specific();
    let channel = options.channel.unwrap_or(Channel::from_version(&version)?);
    let slot = options.slot.clone().unwrap_or(version.slot());
    eprintln!("{version} {channel:?} {slot}");
    let packages = get_platform_extension_packages(channel, &slot, get_server()?)?;
    
    let package = packages.iter().find(|pkg| {
        pkg.tags.get("extension").cloned().unwrap_or_default() == options.extension
    });

    match package {
        Some(pkg) => {
            println!("Found extension package: {} version {}", options.extension, pkg.version);
            // TODO: Implement the installation logic here
        },
        None => {
            return Err(anyhow::anyhow!(
                "Extension '{}' not found in available packages.",
                options.extension
            ));
        }
    }

    Ok(())
}

fn list_extensions(options: &ExtensionListExtensions) -> Result<(), anyhow::Error> {
    let name = match instance_arg(&None, &options.instance)? {
        InstanceName::Local(name) => name,
        inst_name => {
            return Err(anyhow::anyhow!(
                "cannot install extensions in cloud instance {}.",
                inst_name
            ))
            .with_hint(|| {
                format!(
                    "only local instances can install extensions ({} is remote)",
                    inst_name
                )
            })?;
        }
    };
    let Some(inst) = InstanceInfo::try_read(&name)? else {
        return Err(anyhow::anyhow!(
            "cannot install extensions in cloud instance {}.",
            name
        ))
        .with_hint(|| {
            format!(
                "only local instances can install extensions ({} is remote)",
                name
            )
        })?;
    };

    let version = inst.get_version()?.specific();
    let channel = options.channel.unwrap_or(Channel::from_version(&version)?);
    let slot = options.slot.clone().unwrap_or(version.slot());
    eprintln!("{version} {channel:?} {slot}");
    let packages = get_platform_extension_packages(channel, &slot, get_server()?)?;

    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.add_row(row!["Name", "Version"]);
    for pkg in packages {
        let ext = pkg.tags.get("extension").cloned().unwrap_or_default();
        table.add_row(row![ext, pkg.version]);
    }
    table.printstd();
    Ok(())
}
