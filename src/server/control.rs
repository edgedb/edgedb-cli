use std::fs;
use std::path::{Path};

use async_std::task;
use fn_error_context::context;

use crate::credentials;
use crate::server::detect;
use crate::server::options::InstanceCommand;
use crate::server::options::Status;
use crate::server::metadata::Metadata;
use crate::server::methods::Methods;
use crate::server::revert;
use crate::server::status;
use crate::server::os_trait::{InstanceRef};


#[context("failed to read metadata {}/metadata.json", dir.display())]
pub fn read_metadata(dir: &Path) -> anyhow::Result<Metadata> {
    let metadata_path = dir.join("metadata.json");
    Ok(serde_json::from_slice(&fs::read(&metadata_path)?)?)
}

pub fn get_instance<'x>(methods: &'x Methods, name: &str)
    -> anyhow::Result<InstanceRef<'x>>
{
    let mut errors = Vec::new();
    for (meth_name, meth) in methods {
        match meth.get_instance(name) {
            Ok(inst) => return Ok(inst),
            Err(e) => {
                errors.push(format!("  {}: {:#}", meth_name.short_name(), e));
            }
        }
    }
    anyhow::bail!("Cannot find instance {:?}:\n{}", name,
        errors.join("\n"))
}

pub fn instance_command(cmd: &InstanceCommand) -> anyhow::Result<()> {
    use InstanceCommand::*;

    let name = match cmd {
        Start(c) => &c.name,
        Stop(c) => &c.name,
        Restart(c) => &c.name,
        Logs(c) => &c.name,
        Revert(c) => &c.name,
        Status(c) => {
            if let Some(name) = &c.name {
                name
            } else {
                return status::print_status_all(c.extended, c.debug, c.json);
            }
        }
        | Create(_)
        | Destroy(_)
        | Link(_)
        | ResetPassword(_) => {
            unreachable!("handled in server::main::instance_main()");
        }
    };
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let inst = get_instance(&methods, name);
    match cmd {
        Start(c) => inst?.start(c),
        Stop(c) => inst?.stop(c),
        Restart(c) => inst?.restart(c),
        Logs(c) => inst?.logs(c),
        Revert(c) => revert::revert(inst?, c),
        Status(options) => match inst {
            Ok(inst) => local_status(options, inst),
            Err(e) => {
                remote_status(name, options)?;
                Err(e)
            },
        }
        | Create(_)
        | Destroy(_)
        | Link(_)
        | ResetPassword(_) => {
            unreachable!("handled in server::main::instance_main()");
        }
    }
}

fn local_status(options: &Status, inst: InstanceRef) -> anyhow::Result<()> {
    if options.service {
        inst.service_status()
    } else {
        let status = inst.get_status();
        if options.debug {
            println!("{:#?}", status);
            Ok(())
        } else if options.extended {
            status.print_extended_and_exit();
        } else if options.json {
            status.print_json_and_exit();
        } else {
            status.print_and_exit();
        }
    }
}

fn remote_status(name: &String, options: &Status) -> anyhow::Result<()> {
    let path = credentials::path(name)?;
    if !path.exists() {
        return Ok(());
    }
    let status = task::block_on(
        status::RemoteStatus::new(name).probe(path)
    );
    if options.service {
        let path = credentials::path(name)?;
        println!("Remote instance: {}", path.display());
    } else if options.debug {
        println!("{:#?}", status);
    } else if options.extended {
        status.print_extended();
    } else if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status.json())
                .expect("status is json-serializable"),
        );
    } else if let Some(error) = status.status.get_error() {
        println!("{}: {}", status.status.display(), error);
    } else {
        println!("{}", status.status.display());
    }
    status.exit()
}
