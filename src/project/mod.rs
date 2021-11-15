use std::fs;
use std::env;
use std::path::{Path, PathBuf};

use anyhow::Context;

pub mod options;

pub mod config;
pub mod upgrade;
pub mod init;

pub use init::{stash_path, stash_base};

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
