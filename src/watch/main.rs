use std::time::Duration;

use const_format::concatcp;

use edgeql_parser::helpers::quote_string;
use gel_tokio::Error;
use indicatif::ProgressBar;
use notify::RecursiveMode;

use crate::branding::{BRANDING, BRANDING_CLI_CMD};
use crate::connect::{Connection, Connector};
use crate::migrations::{self, dev_mode};
use crate::options::Options;
use crate::portable::project;
use crate::print::AsRelativeToCurrentDir;
use crate::watch::WatchCommand;

#[tokio::main(flavor = "current_thread")]
pub async fn run(options: &Options, cmd: &WatchCommand) -> anyhow::Result<()> {
    if cmd.files {
        return super::files::run().await;
    }

    let project = project::ensure_ctx_async(None).await?;
    let mut ctx = WatchContext {
        connector: options.create_connector().await?,
        migration: migrations::Context::for_project(project)?,
        last_error: false,
    };
    log::info!(
        "Initialized in project dir {}",
        ctx.project().location.root.as_relative().display()
    );

    let mut watcher = super::fs_watcher::FsWatcher::new()?;
    watcher.watch(&ctx.project().location.root, RecursiveMode::NonRecursive)?;
    watcher.watch(&ctx.migration.schema_dir, RecursiveMode::Recursive)?;

    ctx.do_update().await?;

    eprintln!("{BRANDING} Watch initialized.");
    eprintln!("  Hint: Use `{BRANDING_CLI_CMD} migration create` and `{BRANDING_CLI_CMD} migrate --dev-mode` to apply changes once done.");
    eprintln!(
        "Monitoring {}",
        ctx.project().location.root.as_relative().display()
    );

    let mut retry_timeout = None::<Duration>;
    while watcher.wait(retry_timeout).await.is_ok() {
        if let Err(e) = ctx.do_update().await {
            log::error!("Error updating database: {e:#}. Will retry in 10s.");
            retry_timeout = Some(Duration::from_secs(10));
        } else {
            retry_timeout = None;
        }
    }

    // clear error
    let res = ctx.try_connect_and_clear_error().await;
    if let Err(e) = res {
        log::error!("Cannot clear error: {:#}", e);
    }

    Ok(())
}

struct WatchContext {
    connector: Connector,
    migration: migrations::Context,
    last_error: bool,
}

#[derive(serde::Serialize)]
struct ErrorContext {
    line: u32,
    col: u32,
    start: usize,
    end: usize,
    filename: String,
}

#[derive(serde::Serialize)]
struct ErrorJson {
    #[serde(rename = "type")]
    kind: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<ErrorContext>,
}

impl WatchContext {
    async fn do_update(&mut self) -> anyhow::Result<()> {
        let bar = ProgressBar::new_spinner();
        bar.enable_steady_tick(Duration::from_millis(100));
        // TODO(tailhook) check gel/edgedb version
        bar.set_message("connecting");
        let mut cli = self.connector.connect().await?;

        let old_state = cli.set_ignore_error_state();
        let result = dev_mode::migrate(&mut cli, &self.migration, &bar).await;
        cli.restore_state(old_state);

        bar.finish_and_clear();
        match result {
            Ok(()) => {
                if self.last_error {
                    clear_error(&mut cli).await;
                    self.last_error = false;
                    eprintln!("Resolved. Schema is up to date now.");
                }
            }
            Err(e) => {
                eprintln!("Schema migration error: {e:#}");
                set_error(&mut cli, e).await;
                // TODO(tailhook) probably only print if error doesn't match
                self.last_error = true;
            }
        }
        Ok(())
    }
    async fn try_connect_and_clear_error(&mut self) -> anyhow::Result<()> {
        if self.last_error {
            let mut cli = self.connector.connect().await?;
            clear_error(&mut cli).await;
        }
        Ok(())
    }
    fn project(&self) -> &project::Context {
        // SAFETY: watch can only be run within projects.
        // We create Self::migration using migration::Context::for_project
        self.migration.project.as_ref().unwrap()
    }
}

impl From<anyhow::Error> for ErrorJson {
    fn from(err: anyhow::Error) -> ErrorJson {
        if let Some(err) = err.downcast_ref::<Error>() {
            ErrorJson {
                kind: "WatchError",
                message: format!(
                    "error when trying to update the schema.\n  \
                    Original error: {}: {}",
                    err.kind_name(),
                    err.initial_message().unwrap_or(""),
                ),
                hint: Some(
                    concatcp!(
                        "see the window running \
                           `",
                        BRANDING_CLI_CMD,
                        "watch` for more info"
                    )
                    .into(),
                ),
                details: None,
                context: None, // TODO(tailhook)
            }
        } else {
            ErrorJson {
                kind: "WatchError",
                message: format!(
                    "error when trying to update the schema.\n  \
                    Original error: {err}"
                ),
                hint: Some(
                    concatcp!(
                        "see the window running \
                           `",
                        BRANDING_CLI_CMD,
                        " watch` for more info"
                    )
                    .into(),
                ),
                details: None,
                context: None,
            }
        }
    }
}

async fn clear_error(cli: &mut Connection) {
    let res = cli
        .execute("CONFIGURE CURRENT DATABASE RESET force_database_error", &())
        .await;
    let Err(e) = res else { return };
    log::error!("Cannot clear database error state: {:#}", e);
}

async fn set_error(cli: &mut Connection, e: anyhow::Error) {
    let data = serde_json::to_string(&ErrorJson::from(e)).unwrap();
    let res = cli
        .execute(
            &format!(
                "CONFIGURE CURRENT DATABASE SET force_database_error := {}",
                quote_string(&data)
            ),
            &(),
        )
        .await;
    let Err(e) = res else { return };
    log::error!("Cannot set database error state: {:#}", e);
}
