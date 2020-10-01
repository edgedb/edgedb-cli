use std::fs;
use std::path::{Path};

use fn_error_context::context;

use crate::server::detect;
use crate::server::options::InstanceCommand;
use crate::server::metadata::Metadata;
use crate::server::methods::Methods;
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
                errors.push(format!("  {}: {}", meth_name.short_name(), e));
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
        Status(c) => {
            if let Some(name) = &c.name {
                name
            } else {
                return status::print_status_all(c.extended, c.debug);
            }
        }
    };
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let inst = get_instance(&methods, name)?;
    match cmd {
        Start(c) => inst.start(c),
        Stop(c) => inst.stop(c),
        Restart(c) => inst.restart(c),
        Status(options) => {
            if options.service {
                inst.service_status()
            } else {
                let status = inst.get_status();
                if options.debug {
                    println!("{:#?}", status);
                    Ok(())
                } else if options.extended {
                    status.print_extended_and_exit();
                } else {
                    status.print_and_exit();
                }
            }
        }
    }
}
