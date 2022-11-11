use std::time::{Duration, Instant};

use async_std::future::timeout;
use async_std::task;
use edgedb_client::{get_project_dir};
use notify::{RecursiveMode, Watcher};
use tokio::sync::watch;

use crate::options::Options;
use crate::connect::Connector;
use crate::watch::options::WatchCommand;
use crate::migrations::{self, dev_mode};


const STABLE_TIME: Duration = Duration::from_millis(100);

struct WatchContext {
    connector: Connector,
    migration: migrations::Context,
    last_error: bool,
}


pub fn watch(options: &Options, _watch: &WatchCommand)
    -> anyhow::Result<()>
{
    let project_dir = match task::block_on(get_project_dir(None, true))? {
        Some(proj) => proj,
        None => anyhow::bail!("The `edgedb watch` command currently works \
             on projects only. Run `edgedb project init` first."),
    };
    //let mut cli = options.conn_params.connect().await?;
    let mut ctx = WatchContext {
        connector: options.create_connector()?,
        migration: migrations::Context::for_watch(&project_dir)?,
        last_error: false,
    };
    log::info!("Monitoring project dir: {:?}", project_dir);
    task::block_on(ctx.do_update())?;
    let (tx, rx) = watch::channel(());
    let mut watch = notify::recommended_watcher(move |res: Result<_, _>| {
        res.map_err(|e| {
            log::warn!("Error watching filesystem: {:#}", e);
        }).ok();
        tx.send(()).unwrap();
    })?;
    watch.watch(&project_dir.join("edgedb.toml"), RecursiveMode::NonRecursive)?;
    watch.watch(&project_dir.join("dbschema"), RecursiveMode::Recursive)?;
    task::block_on(watch_loop(rx, ctx))?;
    Ok(())
}

async fn watch_loop(mut rx: watch::Receiver<()>, mut ctx: WatchContext)
    -> anyhow::Result<()>
{
    let mut retry_deadline = None::<Instant>;
    loop {
        if let Some(retry_deadline) = retry_deadline {
            let timeo = retry_deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::new(0, 0));
            match timeout(timeo, rx.changed()).await {
                Ok(Ok(())) => {
                    log::debug!("Got change notification. \
                                 Waiting to stabilize.");
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
            log::debug!("Got change notification. Waiting to stabilize.");
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
        if let Err(e) = ctx.do_update().await {
            log::error!("Error updating database: {:#}. \
                         Will retry in 10 sec.", e);
            retry_deadline = Some(Instant::now() + Duration::from_secs(10));
        }
    }
}

impl WatchContext {
    async fn do_update(&mut self) -> anyhow::Result<()> {
        // TODO(tailhook) check edgedb version
        let mut cli = self.connector.connect().await?;
        match dev_mode::migrate(&mut cli, &self.migration).await {
            Ok(()) => {
                if self.last_error {
                    self.last_error = false;
                    eprintln!("Error resolved. Schema is up to date now.");
                }
            }
            Err(e) => {
                // TODO(tailhook) differentiate between temporary errors and
                // errors of user schema or migration errors
                // TODO(tailhook) use database erroring mechanism to notify
                // users
                // TODO(tailhook) better print syntax errors maybe?
                eprintln!("Schema migration error: {e:#}");
                // TODO(tailhook) probably only print if error doesn't match
                self.last_error = true;
            }
        }
        Ok(())
    }
}
