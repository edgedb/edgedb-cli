use std::path;

use crate::portable::{project, windows};
use crate::print::{self, Highlight};

#[tokio::main(flavor = "current_thread")]
pub async fn on_action_sync(
    action: &'static str,
    project: &project::Context,
) -> anyhow::Result<()> {
    on_action(action, project).await
}

pub async fn on_action(action: &'static str, project: &project::Context) -> anyhow::Result<()> {
    let Some(script) = get_hook(action, &project.manifest) else {
        return Ok(());
    };

    print::msg!("{}", format!("hook {action}: {script}").muted());

    let status = run_script(script, &project.location.root).await?;

    // abort on error
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Hook {action} exited with status {status}."
        ));
    }
    Ok(())
}

pub async fn run_script(
    script: &str,
    path: &path::Path,
) -> Result<std::process::ExitStatus, anyhow::Error> {
    let status = if !cfg!(windows) {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .current_dir(path)
            .status()?
    } else {
        let wsl = windows::try_get_wsl()?;
        wsl.sh(path).arg("-c").arg(script).run_for_status().await?
    };
    Ok(status)
}

fn get_hook<'m>(
    action: &'static str,
    manifest: &'m project::manifest::Manifest,
) -> Option<&'m str> {
    let hooks = manifest.hooks.as_ref()?;
    let hook = match action {
        "project.init.before" => &hooks.project.as_ref()?.init.as_ref()?.before,
        "project.init.after" => &hooks.project.as_ref()?.init.as_ref()?.after,
        "branch.switch.before" => &hooks.branch.as_ref()?.switch.as_ref()?.before,
        "branch.switch.after" => &hooks.branch.as_ref()?.switch.as_ref()?.after,
        "branch.wipe.before" => &hooks.branch.as_ref()?.wipe.as_ref()?.before,
        "branch.wipe.after" => &hooks.branch.as_ref()?.wipe.as_ref()?.after,
        "migration.apply.before" => &hooks.migration.as_ref()?.apply.as_ref()?.before,
        "migration.apply.after" => &hooks.migration.as_ref()?.apply.as_ref()?.after,
        "migration.rebase.before" => &hooks.migration.as_ref()?.rebase.as_ref()?.before,
        "migration.rebase.after" => &hooks.migration.as_ref()?.rebase.as_ref()?.after,
        "migration.merge.before" => &hooks.migration.as_ref()?.merge.as_ref()?.before,
        "migration.merge.after" => &hooks.migration.as_ref()?.merge.as_ref()?.after,
        _ => panic!("unknown action"),
    };
    hook.as_deref()
}
