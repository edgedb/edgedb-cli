use std::path::PathBuf;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::platform::home_dir;
use crate::print_markdown;


pub fn check_and_warn() {
    match _check() {
        Ok(None) => {}
        Ok(Some(dir)) => {
            log::warn!("Edgedb CLI has stopped using '{}' for storing data \
                and now uses standard locations of your OS. \
                Run `edgedb self migrate` to update the directory layout.",
                dir.display());
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
            print_markdown!("**edgedb error:** \
                Edgedb CLI has stopped using `${dir}` for storing data \
                and now uses standard locations of your OS. \n\
                To upgrade the directory layout, run: \n\
                ```\n\
                edgedb self migrate\n\
                ```
            ", dir=dir.display());
            return Err(ExitCode::new(11).into());
        }
        None => Ok(())
    }
}
