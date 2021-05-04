use std::fs;
use std::env;
use std::path::{Path, PathBuf};

use anyhow::Context;

pub mod options;

mod main;
pub mod init;
mod unlink;
mod config;

pub use main::main;
pub use init::{stash_path};
pub use unlink::unlink;

#[allow(unused)]  // TODO(tailhook) will be used in `project info`
pub fn project_dir(cli_option: Option<&Path>) -> anyhow::Result<PathBuf> {
    project_dir_opt(cli_option)?
    .ok_or_else(|| {
        anyhow::anyhow!("no `edgedb.toml` found")
    })
}

pub fn project_dir_opt(cli_options: Option<&Path>)
    -> anyhow::Result<Option<PathBuf>>
{
    match cli_options {
        Some(dir) => {
            if dir.join("edgedb.toml").exists() {
                let canon = fs::canonicalize(&dir)
                    .with_context(|| {
                        format!("failed to canonicalize dir {:?}", dir)
                    })?;
                Ok(Some(canon))
            } else {
                anyhow::bail!("no `edgedb.toml` found in {:?}", dir);
            }
        }
        None => {
            let dir = env::current_dir()
                .context("failed to get current directory")?;
            if let Some(ancestor) = init::search_dir(&dir)? {
                let canon = fs::canonicalize(&ancestor)
                    .with_context(|| {
                        format!("failed to canonicalize dir {:?}", ancestor)
                    })?;
                Ok(Some(canon))
            } else {
                Ok(None)
            }
        }
    }
}
