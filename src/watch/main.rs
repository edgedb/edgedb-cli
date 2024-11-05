use std::time::{Duration, Instant};

use edgedb_tokio::{get_project_path, Error};
use edgeql_parser::helpers::quote_string;
use indicatif::ProgressBar;
use notify::{RecursiveMode, Watcher};
use tokio::sync::watch;
use tokio::time::timeout;

use crate::branding::{BRANDING, BRANDING_CLI_CMD};
use crate::connect::{Connection, Connector};
use crate::interrupt::Interrupt;
use crate::migrations::{self, dev_mode};
use crate::options::Options;
use crate::watch::options::WatchCommand;

const STABLE_TIME: Duration = Duration::from_millis(100);

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

pub fn watch(options: &Options, _watch: &WatchCommand) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .thread_name("watch")
        .enable_all()
        .build()?;
    let project_path = match runtime.block_on(get_project_path(None, true))? {
        Some(proj) => proj,
        None => anyhow::bail!(
            "The `{BRANDING_CLI_CMD} watch` command currently only \
             works for projects. Run `{BRANDING_CLI_CMD} project init` first."
        ),
    };
    let mut ctx = WatchContext {
        connector: options.block_on_create_connector()?,
        migration: migrations::Context::for_watch(&project_path)?,
        last_error: false,
    };
    let project_dir = project_path.parent().unwrap();
    log::info!("Initialized in project dir {:?}", project_dir);
    let (tx, rx) = watch::channel(());
    let mut watch = notify::recommended_watcher(move |res: Result<_, _>| {
        res.map_err(|e| {
            log::warn!("Error watching filesystem: {:#}", e);
        })
        .ok();
        tx.send(()).unwrap();
    })?;
    watch.watch(&project_path, RecursiveMode::NonRecursive)?;
    watch.watch(&ctx.migration.schema_dir, RecursiveMode::Recursive)?;

    runtime.block_on(ctx.do_update())?;

    eprintln!("{BRANDING} Watch initialized.");
    eprintln!("  Hint: Use `{BRANDING_CLI_CMD} migration create` and `{BRANDING_CLI_CMD} migrate --dev-mode` to apply changes once done.");
    eprintln!("Monitoring {:?}.", project_dir);
    let res = runtime.block_on(watch_loop(rx, &mut ctx));
    runtime
        .block_on(ctx.try_connect_and_clear_error())
        .map_err(|e| log::error!("Cannot clear error: {:#}", e))
        .ok();
    res
}

pub async fn wait_changes(
    rx: &mut watch::Receiver<()>,
    retry_deadline: Option<Instant>,
) -> anyhow::Result<()> {
    if let Some(retry_deadline) = retry_deadline {
        let timeo = retry_deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::new(0, 0));
        match timeout(timeo, rx.changed()).await {
            Ok(Ok(())) => {
                log::debug!(
                    "Change notification received. \
                             Waiting to stabilize."
                );
            }
            Ok(Err(e)) => {
                anyhow::bail!("error receiving from watch: {:#}", e);
            }
            Err(_) => {
                log::debug!("Retrying...");
            }
        }
    } else {
        rx.changed().await?;
        log::debug!("Change notification received. Waiting to stabilize.");
    }
    loop {
        match timeout(STABLE_TIME, rx.changed()).await {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => {
                anyhow::bail!("error receiving from watch: {:#}", e);
            }
            Err(_) => break,
        }
    }
    Ok(())
}

async fn watch_loop(mut rx: watch::Receiver<()>, ctx: &mut WatchContext) -> anyhow::Result<()> {
    let mut retry_deadline = None::<Instant>;
    loop {
        {
            let ctrl_c = Interrupt::ctrl_c();
            tokio::select! {
                _ = wait_changes(&mut rx, retry_deadline) => (),
                res = ctrl_c.wait_result() => res?,
            };
        }
        retry_deadline = None;
        if let Err(e) = ctx.do_update().await {
            log::error!(
                "Error updating database: {:#}. \
                         Will retry in 10s.",
                e
            );
            retry_deadline = Some(Instant::now() + Duration::from_secs(10));
        }
    }
}

impl WatchContext {
    async fn do_update(&mut self) -> anyhow::Result<()> {
        let bar = ProgressBar::new_spinner();
        bar.enable_steady_tick(Duration::from_millis(100));
        // TODO(tailhook) check edgedb version
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
                    "see the window running \
                           `edgedb watch` for more info"
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
                    Original error: {}",
                    err
                ),
                hint: Some(
                    "see the window running \
                           `edgedb watch` for more info"
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
