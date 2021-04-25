use std::fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::project::options::Unlink;
use crate::project::{project_dir, stash_path};
use crate::question;
use crate::server::destroy;
use crate::server::options::Destroy;

pub fn unlink(options: &Unlink) -> anyhow::Result<()> {
    let dir = project_dir(options.project_dir.as_deref())?;
    let stash_path = stash_path(&dir)?;
    if stash_path.exists() {
        if options.destroy_server_instance {
            let inst = fs::read_to_string(&stash_path.join("instance-name"))
                .context("failed to read instance name")?;
            let inst = inst.trim();
            if !options.non_interactive {
                let q = question::Confirm::new_dangerous(format!(
                    "Do you really want to unlink \
                             and delete instance {:?}?",
                    inst.trim()
                ));
                if !q.ask()? {
                    eprintln!("Canceled");
                    return Ok(());
                }
            }
            let mut project_dirs = destroy::find_project_dirs(inst)?;
            if project_dirs.len() > 1 {
                project_dirs
                    .iter()
                    .position(|d| d == &stash_path)
                    .map(|pos| project_dirs.remove(pos));
                destroy::print_warning(inst, &project_dirs);
                return Err(ExitCode::new(2).into());
            }
            if options.destroy_server_instance {
                destroy::do_destroy(&Destroy {
                    name: inst.to_string(),
                    verbose: false,
                    force: true,
                })?;
            }
            fs::remove_dir_all(&stash_path)?;
        } else {
            match fs::read_to_string(&stash_path.join("instance-name")) {
                Ok(name) => {
                    eprintln!("Unlinking instance {:?}", name);
                }
                Err(e) => {
                    eprintln!("edgedb error: cannot read instance name: {}", e);
                    eprintln!("Removing project configuration directory...");
                }
            };
            fs::remove_dir_all(&stash_path)?;
        }
    }
    Ok(())
}
