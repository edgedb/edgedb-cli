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
    let Some(hook) = get_hook(action, &project.manifest) else {
        return Ok(());
    };

    print::msg!("{}", format!("hook {action}: {hook}").muted());

    // run
    let status = if !cfg!(windows) {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(hook)
            .current_dir(&project.location.root)
            .status()?
    } else {
        let wsl = windows::try_get_wsl()?;
        wsl.sh(&project.location.root)
            .arg("-c")
            .arg(hook)
            .run_for_status()
            .await?
    };

    // abort on error
    if !status.success() {
        return Err(anyhow::anyhow!(
            "Hook {action} exited with status {status}."
        ));
    }
    Ok(())
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
        "schema.update.before" => &hooks.schema.as_ref()?.update.as_ref()?.before,
        "schema.update.after" => &hooks.schema.as_ref()?.update.as_ref()?.after,
        _ => panic!("unknown action"),
    };
    hook.as_deref()
}
