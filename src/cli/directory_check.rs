use std::path::PathBuf;

use anyhow::Context;

use crate::branding::{BRANDING_CLI, BRANDING_CLI_CMD};
use crate::commands::ExitCode;
use crate::platform::home_dir;
use crate::print;
use crate::print_markdown;

pub fn check_and_warn() {
    match _check() {
        Ok(None) => {}
        Ok(Some(dir)) => {
            log::warn!(
                "Edgedb CLI no longer uses '{}' to store data \
                and now uses standard locations of your OS. \
                Run `edgedb cli migrate` to update the directory layout.",
                dir.display()
            );
        }
        Err(e) => log::warn!("Failed directory check: {}", e),
    }
}

fn _check() -> anyhow::Result<Option<PathBuf>> {
    let dir = home_dir()?.join(".edgedb");
    if dir.exists() {
        return Ok(Some(dir));
    }
    Ok(None)
}

pub fn check_and_error() -> anyhow::Result<()> {
    match _check().context("failed directory check")? {
        Some(dir) => {
            print::error!("{BRANDING_CLI} no longer uses `{dir}` to store data \
                and now uses standard locations of your OS.",
                dir = dir.display());
            print_markdown!(
                "To upgrade the directory layout, run: \n\
                ```\n\
                ${cmd} cli migrate\n\
                ```
            ",
                cmd = BRANDING_CLI_CMD
            );
            Err(ExitCode::new(11).into())
        }
        None => Ok(()),
    }
}
