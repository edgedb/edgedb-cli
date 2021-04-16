use std::fs;

use anyhow::Context;

use crate::project::options::Unlink;
use crate::project::{project_dir, stash_path};
use crate::server::destroy;
use crate::server::options::Destroy;
use crate::question;


pub fn unlink(options: &Unlink) -> anyhow::Result<()> {
    let dir = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let stash_path = stash_path(&dir)?;
    if stash_path.exists() {
        if options.destroy_server_instance {
            let inst = fs::read_to_string(&stash_path.join("instance-name"))
                .context("failed to read instance name")?;
            if !options.non_interactive {
                let q = question::Confirm::new_dangerous(
                    format!("Do you really want to unlink \
                             and delete instance {:?}?", inst.trim())
                );
                if !q.ask()? {
                    eprintln!("Canceled");
                    return Ok(())
                }
            }
            if options.destroy_server_instance {
                destroy::destroy(&Destroy {
                    name: inst.trim().to_string(),
                    verbose: false,
                })?;
            }
            fs::remove_dir_all(&stash_path)?;
        } else {
            match fs::read_to_string(&stash_path.join("instance-name")) {
                Ok(name) => {
                    eprintln!("Unlinking instance {:?}", name);
                }
                Err(e) => {
                    eprintln!("edgedb error: cannot read instance name: {}",
                              e);
                    eprintln!("Removing project configuration directory...");
                }
            };
            fs::remove_dir_all(&stash_path)?;
        }
    }
    Ok(())
}
