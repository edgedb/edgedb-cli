use crate::{portable::project::manifest, print};

pub fn on_action(action: &'static str, manifest: &manifest::Manifest) -> anyhow::Result<()> {
    let Some(hooks) = get_hook(action, manifest) else {
        return Ok(());
    };

    // parse
    let parsed_hooks = hooks
        .iter()
        .map(|h| shell_words::split(h))
        .collect::<Result<Vec<Vec<String>>, _>>()?;

    for (hook, args) in std::iter::zip(hooks, parsed_hooks) {
        print::msg!("hook {action}: {hook}");

        // run
        let (program, args) = args.split_first().unwrap();
        let status = std::process::Command::new(program).args(args).status()?;

        // abort on error
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Hook {action} exited with status {status}."
            ));
        }
    }

    Ok(())
}

fn get_hook<'m>(action: &'static str, manifest: &'m manifest::Manifest) -> Option<&'m [String]> {
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
    Some(hook.as_ref()?.as_ref())
}
