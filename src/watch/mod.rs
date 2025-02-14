mod fs_watcher;
mod migrate;

use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;

pub use fs_watcher::{Event, FsWatcher};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;

#[allow(unused_imports)]
use crate::branding::{BRANDING_CLI_CMD, MANIFEST_FILE_DISPLAY_NAME};
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

    #[arg(short = 'v', long)]
    pub verbose: bool,
}

#[tokio::main(flavor = "current_thread")]
pub async fn run(options: &Options, cmd: &Command) -> anyhow::Result<()> {
    let project = Arc::new(project::ensure_ctx_async(None).await?);

    // determine what we will be watching
    let matchers = assemble_matchers(cmd, &project)?;

    if cmd.migrate {
        print::msg!(
            "Hint: --migrate will apply any changes from your schema files to the database. \
            When ready to commit your changes, use:"
        );
        print::msg!(
            "1) `{BRANDING_CLI_CMD} migration create` to write those changes to a migration file,"
        );
        print::msg!(
            "2) `{BRANDING_CLI_CMD} migrate --dev-mode` to replace all synced \
            changes with the migration.\n"
        );
    }

    print::msg!(
        "{} {} for changes in:",
        "Monitoring".emphasized(),
        project.location.root.as_relative().display()
    );
    print::msg!("");
    for m in &matchers {
        print::msg!("  {}: {}", m.glob, m.target.to_string().muted());
    }
    print::msg!("");

    let mut watcher = fs_watcher::FsWatcher::new()?;
    // TODO: watch only directories that are needed, not the whole project

    watcher.watch(&project.location.root, notify::RecursiveMode::Recursive)?;
    let schema_dir = project.manifest.project().get_schema_dir();
    if cmd.migrate && !schema_dir.starts_with(&project.location.root) {
        watcher.watch(
            &project.manifest.project().get_schema_dir(),
            notify::RecursiveMode::Recursive,
        )?;
    }

    let (tx, join_handle) = start_executors(&matchers, options, &project).await?;

    loop {
        let timeout = None;

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
            .flat_map(|p| p.strip_prefix(&project.location.root).ok())
            .map(|p| (p, globset::Candidate::new(p)))
            .collect();

        // run all matching scripts
        for (matcher, tx) in std::iter::zip(&matchers, &tx) {
            // does it match?
            let matched_paths = changed_paths
                .iter()
                .filter(|x| matcher.matcher.is_match_candidate(&x.1))
                .map(|x| x.0.display().to_string())
                .collect::<Vec<_>>();
            if matched_paths.is_empty() {
                continue;
            }

            let order = ExecutionOrder {
                matched_paths: HashSet::from_iter(matched_paths),
            };
            tx.send(order).unwrap();
        }
    }

    // close all tx
    for t in tx {
        drop(t);
    }
    // wait for executors to finish
    join_handle.join_all().await;

    Ok(())
}

struct Matcher {
    glob: String,
    matcher: globset::GlobMatcher,
    target: Target,
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

fn assemble_matchers(
    cmd: &Command,
    project: &project::Context,
) -> anyhow::Result<Vec<Arc<Matcher>>> {
    let watch = project.manifest.watch.as_ref();
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

        matchers.push(Arc::new(Matcher {
            glob: glob_str,
            matcher: glob.compile_matcher(),
            target: Target::Script(script),
        }));
    }
    matchers.sort_by(|a, b| b.glob.cmp(&a.glob));

    if cmd.migrate {
        let schema_dir = project.manifest.project().get_schema_dir();
        let schema_dir = schema_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("bad path: {}", schema_dir.display()))?;
        let glob_str = format!("{schema_dir}/**/*.{{gel,esdl}}");
        let glob = globset::Glob::new(&glob_str)?;
        matchers.push(Arc::new(Matcher {
            glob: glob_str,
            matcher: glob.compile_matcher(),
            target: Target::MigrateDevMode,
        }));
    }

    Ok(matchers)
}

struct ExecutionOrder {
    matched_paths: HashSet<String>,
}

impl ExecutionOrder {
    fn merge(&mut self, other: ExecutionOrder) {
        self.matched_paths.extend(other.matched_paths);
    }

    async fn recv(input: &mut UnboundedReceiver<ExecutionOrder>) -> Option<ExecutionOrder> {
        let mut order = input.recv().await?;
        loop {
            match input.try_recv() {
                Ok(o) => order.merge(o),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return None,
            }
        }
        Some(order)
    }

    fn print(&self, matcher: &Matcher) {
        // print
        print::msg!(
            "{}",
            format!(
                "--- {}: {} ---",
                matcher.glob,
                matcher.target.to_string().muted()
            )
        );
        // if cmd.verbose {
        //     print::msg!("{}", format!("  triggered by: {reason}").muted());
        // }
    }
}

async fn start_executors(
    matchers: &[Arc<Matcher>],
    options: &Options,
    project: &Arc<project::Context>,
) -> anyhow::Result<(Vec<UnboundedSender<ExecutionOrder>>, JoinSet<()>)> {
    let mut senders = Vec::with_capacity(matchers.len());
    let mut join_set = JoinSet::new();
    for matcher in matchers {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        senders.push(tx);

        match &matcher.target {
            Target::Script(script) => join_set.spawn(execute_scripts(
                rx,
                matcher.clone(),
                script.clone(),
                project.clone(),
            )),
            Target::MigrateDevMode => {
                let connector = options.create_connector().await?;
                let migrator = migrate::Migrator::new(connector, project.clone())?;

                join_set.spawn(migrator.run(rx, matcher.clone()))
            }
        };
    }
    Ok((senders, join_set))
}

async fn execute_scripts(
    mut input: UnboundedReceiver<ExecutionOrder>,
    matcher: Arc<Matcher>,
    script: String,
    project: Arc<project::Context>,
) {
    while let Some(order) = ExecutionOrder::recv(&mut input).await {
        order.print(&matcher);

        let res = crate::hooks::run_script(&script, &project.location.root).await;

        match res {
            Ok(status) => {
                if !status.success() {
                    print::error!("script exited with status {status}");
                }
            }
            Err(e) => {
                print::error!("{e}")
            }
        }
    }
}
