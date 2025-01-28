use crate::portable::{project, windows};
use crate::print::{self, Highlight};

pub fn on_action(action: &'static str, project: &project::Context) -> anyhow::Result<()> {
    let Some(hook) = get_hook(action, &project.manifest) else {
        return Ok(());
    };

    print::msg!("{}", format!("hook {action}: {hook}").fade());

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
            .status()?
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
        "migration.rebase.before" => &hooks.migration.as_ref()?.rebase.as_ref()?.before,
        "migration.rebase.after" => &hooks.migration.as_ref()?.rebase.as_ref()?.after,
        "migration.merge.before" => &hooks.migration.as_ref()?.merge.as_ref()?.before,
        "migration.merge.after" => &hooks.migration.as_ref()?.merge.as_ref()?.after,
        _ => panic!("unknown action"),
    };
    hook.as_deref()
}
