use std::fs;
use std::path::{Path};

use fn_error_context::context;

use crate::credentials;
use crate::hint::HintExt;
use crate::server::detect;
use crate::server::link;
use crate::server::options::InstanceCommand;
use crate::server::metadata::Metadata;
use crate::server::methods::Methods;
use crate::server::revert;
use crate::server::status;
use crate::server::os_trait::{InstanceRef};
use crate::server::errors::InstanceNotFound;


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
                errors.push((meth_name.short_name(), e));
            }
        }
    }
    let err = anyhow::anyhow!("Cannot find instance {:?}:\n{}",
        name,
        errors.iter().map(|(n, e)| format!("  {}: {:#}", n, e))
            .collect::<Vec<_>>().join("\n")
    );
    if errors.iter().all(|(_, e)| e.is::<InstanceNotFound>()) {
        return Err(InstanceNotFound(err).into());
    } else {
        return Err(err);
    }
}

pub fn instance_command(cmd: &InstanceCommand) -> anyhow::Result<()> {
    use InstanceCommand::*;

    let name = match cmd {
        Start(c) => &c.name,
        Stop(c) => &c.name,
        Restart(c) => &c.name,
        Logs(c) => &c.name,
        Revert(c) => &c.name,
        Unlink(c) => &c.name,
        Status(c) => &c.name,
        | Create(_)
        | Destroy(_)
        | Link(_)
        | List(_)
        | Upgrade(_)
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
        Status(c) => match inst {
            Ok(inst) => status::print_status_local(
                inst, c.service, c.debug, c.extended, c.json
            ),
            Err(e) => if credentials::path(name)?.exists() {
                status::print_status_remote(
                    name, c.service, c.debug, c.extended, c.json
                )
            } else {
                Err(e)
            },
        }
        Unlink(_) => if inst.is_err() {
            link::unlink(name)
        } else {
            Err(anyhow::anyhow!("Cannot unlink a local instance."))
                .hint("Use `instance destroy` instead.")?
        },
        | Create(_)
        | Destroy(_)
        | Link(_)
        | List(_)
        | Upgrade(_)
        | ResetPassword(_) => {
            unreachable!("handled in server::main::instance_main()");
        }
    }
}
