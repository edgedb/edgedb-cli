use std::path::{Path, PathBuf};
use std::env;
use std::fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::print;
use crate::project::options::Unlink;
use crate::project::{stash_path};
use crate::server::destroy;
use crate::server::options::Destroy;
use crate::question;

fn search_dir(base: &Path) -> anyhow::Result<PathBuf> {
    let mut path = base;
    let canon = fs::canonicalize(&path)
        .with_context(|| format!("failed to canonicalize dir {:?}", path))?;
    let stash_dir = stash_path(&canon)?;
    if stash_dir.exists() || path.join("edgedb.toml").exists() {
        return Ok(stash_dir)
    }
    while let Some(parent) = path.parent() {
        let canon = fs::canonicalize(&parent)
            .with_context(|| {
                format!("failed to canonicalize dir {:?}", parent)
            })?;
        let stash_dir = stash_path(&canon)?;
        if stash_dir.exists() || path.join("edgedb.toml").exists() {
            return Ok(stash_dir)
        }
        path = parent;
    }
    anyhow::bail!("no project directory found");
}

pub fn unlink(options: &Unlink) -> anyhow::Result<()> {
    let stash_path = if let Some(dir) = &options.project_dir {
        let canon = fs::canonicalize(&dir)
            .with_context(|| format!("failed to canonicalize dir {:?}", dir))?;
        stash_path(&canon)?
    } else {
        let base = env::current_dir()
            .context("failed to get current directory")?;
        search_dir(&base)?
    };

    if stash_path.exists() {
        if options.destroy_server_instance {
            let inst = fs::read_to_string(&stash_path.join("instance-name"))
                .context("failed to read instance name")?;
            let inst = inst.trim();
            if !options.non_interactive {
                let q = question::Confirm::new_dangerous(
                    format!("Do you really want to unlink \
                             and delete instance {:?}?", inst.trim())
                );
                if !q.ask()? {
                    print::error("Canceled.");
                    return Ok(())
                }
            }
            let mut project_dirs = destroy::find_project_dirs(inst)?;
            if project_dirs.len() > 1 {
                project_dirs.iter().position(|d| d == &stash_path)
                    .map(|pos| project_dirs.remove(pos));
                destroy::print_warning(inst, &project_dirs);
                return Err(ExitCode::new(2))?;
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
                    print::error_msg(
                        "edgedb error",
                        &format!("cannot read instance name: {}", e),
                    );
                    eprintln!("Removing project configuration directory...");
                }
            };
            fs::remove_dir_all(&stash_path)?;
        }
    } else {
        log::warn!("no project directory exists");
    }
    Ok(())
}
