use crate::branding::MANIFEST_FILE_DISPLAY_NAME;
use crate::portable::project;
use crate::print::{self, AsRelativeToCurrentDir, Highlight};

pub async fn run() -> anyhow::Result<()> {
    let project = project::ensure_ctx_async(None).await?;

    let watch = project.manifest.watch.as_ref();
    let Some(files) = watch.and_then(|x| x.files.as_ref()) else {
        print::error!(
            "no watch paths defined in [watch.files] in {}",
            MANIFEST_FILE_DISPLAY_NAME
        );
        return Ok(());
    };

    let mut globs = Vec::new();
    for (glob_str, script) in files {
        let glob = globset::Glob::new(&glob_str)?;

        globs.push((glob_str, glob.compile_matcher(), script));
    }
    globs.sort_by_key(|x| x.0.as_str());

    let mut watcher = super::fs_watcher::FsWatcher::new()?;

    // TODO: watch only directories that are needed, not the whole project
    print::msg!(
        "Monitoring {} for changes in:",
        project.location.root.as_relative().display()
    );
    for (glob, _, _) in &globs {
        print::msg!("- {glob}");
    }
    watcher.watch(&project.location.root, notify::RecursiveMode::Recursive)?;

    loop {
        // wait for changes
        let changed_paths = watcher.wait(None).await?;
        if changed_paths.is_empty() {
            break;
        }
        // strip prefix
        let changed_paths: Vec<_> = changed_paths
            .iter()
            .flat_map(|p| p.strip_prefix(&project.location.root).ok())
            .collect();

        // run all matching scripts
        for (glob_str, matcher, script) in &globs {
            let is_matched = changed_paths.iter().any(|x| matcher.is_match(x));
            if !is_matched {
                continue;
            }

            print::msg!("{}", format!("--- watch.files \"{glob_str}\" ---").muted());
            print::msg!("{}", format!("  $ {script}").muted());

            let status = crate::hooks::run_script(script, &project.location.root).await?;

            if !status.success() {
                print::error!("script exited with status {status}");
            }
        }
    }
    Ok(())
}
