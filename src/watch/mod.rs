mod fs_watcher;
mod migrate;

pub use fs_watcher::{Event, FsWatcher};

#[allow(unused_imports)]
use crate::branding::{BRANDING_CLI_CMD, MANIFEST_FILE_DISPLAY_NAME};
use crate::connect::Connector;
use crate::options::Options;
use crate::portable::project;
use crate::print::{self, AsRelativeToCurrentDir, Highlight};

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    /// Runs "[BRANDING_CLI_CMD] migration apply --dev-mode" on changes to schema definitions.
    /// On migration errors `force_database_error` is set, which rejects all queries
    /// to the database. This configuration is cleared when the error is resolved or watch is stopped.
    ///
    /// This runs in addition to to scripts in [MANIFEST_FILE_DISPLAY_NAME].
    #[arg(short = 'm', long)]
    pub migrate: bool,

    /// On failure, retry after a given number of seconds
    #[arg(short = 'r', long)]
    pub retry_sec: Option<u16>,

    /// Don't suspend watch during script execution. Might cause loops.
    #[arg(short = 'c', long)]
    pub continuous: bool,

    #[arg(short = 'v', long)]
    pub verbose: bool,
}

#[tokio::main(flavor = "current_thread")]
pub async fn run(options: &Options, cmd: &Command) -> anyhow::Result<()> {
    let project = project::ensure_ctx_async(None).await?;
    let mut ctx = WatchContext {
        project,
        connector: options.create_connector().await?,
        is_force_database_error: false,
    };

    // determine what we will be watching
    let mut matchers = assemble_matchers(cmd, &ctx)?;

    if cmd.migrate {
        print::msg!(
            "Hint: --migrate will apply any changes from your schema files to the database. \
            When ready to commit your changes, use:"
        );
        print::msg!(
            "1) `{BRANDING_CLI_CMD} migration create` to write those changes to a migration file,"
        );
        print::msg!(
            "2) `{BRANDING_CLI_CMD} migration apply --dev-mode` to replace all synced \
            changes with the migration.\n"
        );
    }

    print::msg!(
        "{} {} for changes in:",
        "Monitoring".emphasized(),
        ctx.project.location.root.as_relative().display()
    );
    print::msg!("");
    for m in &matchers {
        print::msg!("  {}: {}", m.glob, m.target.to_string().muted());
    }
    print::msg!("");

    let mut watcher = fs_watcher::FsWatcher::new()?;
    // TODO: watch only directories that are needed, not the whole project

    watcher.watch(&ctx.project.location.root, notify::RecursiveMode::Recursive)?;
    let schema_dir = ctx.project.manifest.project().get_schema_dir();
    if cmd.migrate && !schema_dir.starts_with(&ctx.project.location.root) {
        watcher.watch(
            &ctx.project.manifest.project().get_schema_dir(),
            notify::RecursiveMode::Recursive,
        )?;
    }

    loop {
        let timeout = if let Some(retry_sec) = &cmd.retry_sec {
            let any_failed = matchers.iter().any(|m| m.last_failed);
            any_failed.then(|| tokio::time::Duration::from_secs(*retry_sec as u64))
        } else {
            None
        };
        if let Some(timeout) = timeout {
            print::warn!("Retrying in {} sec...", timeout.as_secs());
        }

        if !cmd.continuous {
            watcher.clear_queue();
        }

        // wait for changes
        let event = watcher.wait(timeout).await;

        let changed_paths = match event {
            Event::Changed(paths) => paths,
            Event::Retry => Default::default(),
            Event::Abort => break,
        };
        // strip prefix
        let changed_paths: Vec<_> = changed_paths
            .iter()
            .flat_map(|p| p.strip_prefix(&ctx.project.location.root).ok())
            .map(|p| (p, globset::Candidate::new(p)))
            .collect();

        // run all matching scripts
        for matcher in &mut matchers {
            // does it match?
            let (is_matched, reason) = if matcher.last_failed {
                (true, "(retry)".to_string())
            } else {
                let matched_paths = changed_paths
                    .iter()
                    .filter(|x| matcher.matcher.is_match_candidate(&x.1))
                    .map(|x| x.0.display().to_string())
                    .collect::<Vec<_>>();
                (!matched_paths.is_empty(), matched_paths.join(", "))
            };

            if !is_matched {
                continue;
            }

            // print
            print::msg!(
                "{}",
                format!(
                    "--- {}: {} ---",
                    matcher.glob,
                    matcher.target.to_string().muted()
                )
            );
            if cmd.verbose {
                print::msg!("{}", format!("  triggered by: {reason}").muted());
            }

            // run
            let success = match &matcher.target {
                Target::Script(script) => {
                    let status =
                        crate::hooks::run_script(script, &ctx.project.location.root).await?;

                    if !status.success() {
                        print::error!("script exited with status {status}");
                    }
                    status.success()
                }
                Target::MigrateDevMode => {
                    let res = ctx.migration_apply_dev_mode().await;

                    if let Err(e) = &res {
                        print::error!("{e}");
                    }
                    res.is_ok()
                }
            };

            matcher.last_failed = !success;
        }

        print::msg!(""); // a bit of space between runs
    }

    if cmd.migrate {
        ctx.cleanup().await;
    }

    Ok(())
}

struct WatchContext {
    project: project::Context,

    // things needed for migrate
    connector: Connector,
    is_force_database_error: bool,
}

struct Matcher {
    glob: String,
    matcher: globset::GlobMatcher,
    target: Target,
    last_failed: bool,
}

enum Target {
    Script(String),
    MigrateDevMode,
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Script(s) => f.write_str(s),
            Target::MigrateDevMode => {
                f.write_str(BRANDING_CLI_CMD)?;
                f.write_str(" migration apply --dev-mode")
            }
        }
    }
}

fn assemble_matchers(cmd: &Command, ctx: &WatchContext) -> anyhow::Result<Vec<Matcher>> {
    let watch = ctx.project.manifest.watch.as_ref();
    let files = match watch.and_then(|x| x.files.as_ref()) {
        Some(files) => files.clone(),
        None if cmd.migrate => Default::default(),
        None => {
            return Err(anyhow::anyhow!(
                "[watch.files] table missing in {}",
                MANIFEST_FILE_DISPLAY_NAME
            ));
        }
    };

    let mut matchers = Vec::new();
    for (glob_str, script) in files {
        let glob = globset::Glob::new(&glob_str)?;

        matchers.push(Matcher {
            glob: glob_str,
            matcher: glob.compile_matcher(),
            target: Target::Script(script),
            last_failed: false,
        });
    }
    matchers.sort_by(|a, b| b.glob.cmp(&a.glob));

    if cmd.migrate {
        let schema_dir = ctx.project.manifest.project().get_schema_dir();
        let schema_dir = schema_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("bad path: {}", schema_dir.display()))?;
        let glob_str = format!("{schema_dir}/**/*.{{gel,esdl}}");
        let glob = globset::Glob::new(&glob_str)?;
        matchers.push(Matcher {
            glob: glob_str,
            matcher: glob.compile_matcher(),
            target: Target::MigrateDevMode,
            last_failed: false,
        });
    }

    Ok(matchers)
}
