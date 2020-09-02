use std::fs;
use std::process::Command;

use anyhow::Context;

use crate::server::init;
use crate::server::linux;
use crate::server::macos;
use crate::server::package::Package;


pub fn bootstrap(settings: &init::Settings)
    -> anyhow::Result<()>
{
    fs::create_dir_all(&settings.directory)
        .with_context(|| format!("failed to create {}",
                                 settings.directory.display()))?;

    let pkg = settings.distribution.downcast_ref::<Package>()
        .context("invalid unix package")?;
    let mut cmd = Command::new(if cfg!(target_os="macos") {
        macos::get_server_path(&pkg.slot)
    } else {
        linux::get_server_path(Some(&pkg.slot))
    });
    cmd.arg("--bootstrap");
    cmd.arg("--log-level=warn");
    cmd.arg("--data-dir").arg(&settings.directory);
    if settings.inhibit_user_creation {
        cmd.arg("--default-database=edgedb");
        cmd.arg("--default-database-user=edgedb");
    }

    log::debug!("Running bootstrap {:?}", cmd);
    match cmd.status() {
        Ok(s) if s.success() => {}
        Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
        Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
    }
    Ok(())
}
