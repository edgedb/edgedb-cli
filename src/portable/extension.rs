use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::Context;
use edgedb_protocol::QueryResult;
use log::trace;
use prettytable::{row, Table};

use super::options::{ExtensionInstall, ExtensionList, ExtensionListAvailable, ExtensionUninstall};

use crate::branding::BRANDING_CLOUD;
use crate::connect::Connector;
use crate::hint::HintExt;
use crate::options::Options;
use crate::portable::install::download_package;
use crate::portable::local::InstanceInfo;
use crate::portable::options::{instance_arg, ExtensionCommand, InstanceName};
use crate::portable::platform::get_server;
use crate::portable::repository::{get_platform_extension_packages, Channel};
use crate::table;

pub fn extension_main(c: &ExtensionCommand, o: &Options) -> Result<(), anyhow::Error> {
    use crate::portable::options::InstanceExtensionCommand::*;
    match &c.subcommand {
        Install(c) => install(c, o),
        List(c) => list(c, o),
        ListAvailable(c) => list_available(c, o),
        Uninstall(c) => uninstall(c, o),
    }
}

fn get_local_instance(options: &Options) -> Result<InstanceInfo, anyhow::Error> {
    let instance = &options.conn_options.instance;

    let name = match instance_arg(&None, instance)? {
        InstanceName::Local(name) => name,
        inst_name => {
            return Err(anyhow::anyhow!(
                "cannot install extensions in {BRANDING_CLOUD} instance {}.",
                inst_name
            ))
            .with_hint(|| {
                format!("only local instances can install extensions ({inst_name} is remote)")
            })?;
        }
    };
    let Some(inst) = InstanceInfo::try_read(name)? else {
        return Err(anyhow::anyhow!(
            "cannot install extensions in {BRANDING_CLOUD} instance {}.",
            name
        ))
        .with_hint(|| format!("only local instances can install extensions ({name} is remote)"))?;
    };
    Ok(inst)
}

type ExtensionInfo = (String, String);

fn _get_extensions(options: &Options) -> Result<Vec<ExtensionInfo>, anyhow::Error> {
    if let InstanceName::Local(name) = instance_arg(&None, &options.conn_options.instance)? {
        // if local instance, check instance info
        let instance_info = InstanceInfo::try_read(name)?;
        if let Some(instance_info) = instance_info {
            let extension_loader = instance_info.extension_loader_path()?;
            let output =
                run_extension_loader(&extension_loader, Some("--list-packages"), None::<&str>)?;
            let value: serde_json::Value = serde_json::from_str(&output)?;

            let mut extensions: Vec<ExtensionInfo> = vec![];
            if let Some(array) = value.as_array() {
                for pkg in array {
                    let name = pkg
                        .get("extension_name")
                        .map(|s| s.as_str().unwrap_or_default().to_owned())
                        .unwrap_or_default();
                    let version = pkg
                        .get("extension_version")
                        .map(|s| s.as_str().unwrap_or_default().to_owned())
                        .unwrap_or_default();
                    extensions.push((name, version));
                }
            }

            return Ok(extensions);
        }
    }

    // if remote or cloud instance, connect and query extension packages
    let query = "for ext in sys::ExtensionPackage union (
        with
            ver := ext.version,
            ver_str := <str>ver.major++'.'++<str>ver.minor,
        select (ext.name, ver_str)
    );";

    let connector = options.block_on_create_connector()?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let extension_query = runtime.spawn(connector.run_single_query::<ExtensionInfo>(query));

    let extensions = runtime.block_on(extension_query)??;
    Ok(extensions)
}

fn list(_: &ExtensionList, options: &Options) -> Result<(), anyhow::Error> {
    let extensions = _get_extensions(options)?;

    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.set_titles(row!["Name", "Version"]);
    for (name, version) in extensions {
        table.add_row(row![name, version]);
    }
    table.printstd();

    Ok(())
}

fn uninstall(uninstall: &ExtensionUninstall, options: &Options) -> Result<(), anyhow::Error> {
    let inst = get_local_instance(options)?;
    let extension_loader = inst.extension_loader_path()?;
    run_extension_loader(
        &extension_loader,
        Some("--uninstall".to_string()),
        Some(Path::new(&uninstall.extension)),
    )?;
    Ok(())
}

fn install(install: &ExtensionInstall, options: &Options) -> Result<(), anyhow::Error> {
    let inst = get_local_instance(options)?;
    let extension_loader = inst.extension_loader_path()?;

    let version = inst.get_version()?.specific();
    let channel = install.channel.unwrap_or(Channel::from_version(&version)?);
    let slot = install.slot.clone().unwrap_or(version.slot());
    trace!("Instance: {version} {channel:?} {slot}");
    let packages = get_platform_extension_packages(channel, &slot, get_server()?)?;

    let package = packages
        .iter()
        .find(|pkg| pkg.tags.get("extension").cloned().unwrap_or_default() == install.extension);

    match package {
        Some(pkg) => {
            println!(
                "Found extension package: {} version {}",
                install.extension, pkg.version
            );
            let zip = download_package(pkg)?;
            let command = if install.reinstall {
                Some("--reinstall")
            } else {
                None
            };
            run_extension_loader(&extension_loader, command, Some(&zip))?;
            println!("Extension '{}' installed successfully.", install.extension);
        }
        None => {
            return Err(anyhow::anyhow!(
                "Extension '{}' not found in available packages.",
                install.extension
            ));
        }
    }

    Ok(())
}

fn run_extension_loader(
    extension_installer: &Path,
    command: Option<impl AsRef<OsStr>>,
    file: Option<impl AsRef<OsStr>>,
) -> Result<String, anyhow::Error> {
    let mut cmd = Command::new(extension_installer);

    if let Some(cmd_str) = command {
        cmd.arg(cmd_str);
    }

    if let Some(file_path) = file {
        cmd.arg(file_path);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {}", extension_installer.display()))?;

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
        return Err(anyhow::anyhow!(
            "Extension installation failed with exit code: {}",
            output.status
        ));
    } else {
        trace!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
        trace!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn list_available(list: &ExtensionListAvailable, options: &Options) -> Result<(), anyhow::Error> {
    let inst = get_local_instance(options)?;

    let version = inst.get_version()?.specific();
    let channel = list.channel.unwrap_or(Channel::from_version(&version)?);
    let slot = list.slot.clone().unwrap_or(version.slot());
    trace!("Instance: {version} {channel:?} {slot}");
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
