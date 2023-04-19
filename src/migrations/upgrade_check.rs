use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use edgedb_tokio::Builder;
use indicatif::ProgressBar;
use notify::{RecursiveMode, Watcher};
use tokio::fs;
use tokio::sync::watch;

use crate::async_try;
use crate::commands::{Options, ExitCode};
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::create::{execute_start_migration, EsdlError};
use crate::migrations::edb::{execute, execute_if_connected};
use crate::migrations::migrate::{apply_migration, ApplyMigrationError};
use crate::migrations::migration;
use crate::migrations::options::UpgradeCheck;
use crate::migrations::timeout;
use crate::portable::install;
use crate::portable::repository::{self, Query};
use crate::print::{warn, success, echo, Highlight};
use crate::process;
use crate::watch::wait_changes;

#[derive(Debug, serde::Deserialize)]
struct EdgedbStatus {
    port: u16,
    tls_cert_file: String,
}

enum CheckResult {
    Okay,
    SchemaIssue,
    MigrationsIssue,
}


pub fn upgrade_check(_options: &Options, options: &UpgradeCheck)
    -> anyhow::Result<()>
{
    let (version, _) = Query::from_options(
        repository::QueryOptions {
            nightly: options.to_nightly,
            stable: false,
            testing: options.to_testing,
            version: options.to_version.as_ref(),
            channel: options.to_channel,
        },
        || Ok(Query::stable()))?;
    if cfg!(windows) {
        todo!();
    } else {
        use tokio::net::UnixDatagram;

        let pkg = repository::get_server_package(&version)?
            .with_context(|| format!("no package matching {} found",
                                     version.display()))?;
        let info = install::package(&pkg).context("error installing EdgeDB")?;
        let server_path = info.server_path()?;

        let status_dir = tempfile::tempdir().context("tempdir failure")?;
        let mut cmd = process::Native::new("edgedb", "edgedb", server_path);
        cmd.env("NOTIFY_SOCKET", &status_dir.path().join("notify"));
        cmd.arg("--temp-dir");
        cmd.arg("--auto-shutdown-after=0");
        cmd.arg("--default-auth-method=Trust");
        cmd.arg("--emit-server-status").arg(&status_dir.path().join("status"));
        cmd.arg("--port=auto");
        cmd.arg("--compiler-pool-mode=on_demand");
        cmd.arg("--tls-cert-mode=generate_self_signed");
        cmd.arg("--log-level=warn");
        cmd.background_for(move || {
            // this is not async, but requires async context
            let sock = UnixDatagram::bind(&status_dir.path().join("notify"))
                .context("cannot create notify socket")?;
            Ok(async move {
                let ctx = Context::from_project_or_config(
                    &options.cfg, true,
                ).await?;
                let mut buf = [0u8; 1024];
                while !matches!(sock.recv(&mut buf).await,
                               Ok(len) if &buf[..len] == b"READY=1")
                { };

                let status_file = status_dir.path().join("status");
                do_check(&ctx, &status_file, options.watch).await
            })
        })
    }
}


async fn do_check(ctx: &Context, status_file: &Path, watch: bool)
    -> anyhow::Result<()>
{
    use CheckResult::*;

    let status_data = fs::read_to_string(&status_file).await
        .context("error reading status")?;
    let Some(json_data) = status_data.strip_prefix("READY=") else {
        anyhow::bail!("Invalid server status {status_data:?}");
    };
    let status: EdgedbStatus = serde_json::from_str(json_data)?;
    let cert_data = fs::read_to_string(&status.tls_cert_file).await?;
    let config = Builder::new()
        .port(status.port)?
        .pem_certificates(&cert_data)?
        .constrained_build()
        .context("cannot build connection params")?;
    let cli = &mut Connection::connect(&config).await?;

    if watch {
        let (tx, rx) = watch::channel(());
        let mut watch = notify::recommended_watcher(move |res: Result<_, _>| {
            res.map_err(|e| {
                log::warn!("Error watching filesystem: {:#}", e);
            }).ok();
            tx.send(()).unwrap();
        })?;
        // TODO(tailhook) do we have to monitor `edgedb.toml` for the schema
        // dir change
        watch.watch(&ctx.schema_dir, RecursiveMode::Recursive)?;

        let ok = matches!(single_check(ctx, cli).await?, Okay);
        if ok {
            success("The schema is forward-compatible. Ready for upgrade.");
        }
        eprintln!("Monitoring {:?} for changes.", &ctx.schema_dir);
        watch_loop(rx, ctx, cli, ok).await?;
        return Ok(());
    } else {
        match single_check(ctx, cli).await? {
            Okay => {}
            SchemaIssue => {
                echo!("For faster feedback loop use:");
                echo!("    edgedb migration upgrade-check --watch"
                      .command_hint());
                return Err(ExitCode::new(3))?;
            }
            MigrationsIssue => {
                // Should be no need to watch
                return Err(ExitCode::new(4))?;
            }
        }
        if !ctx.quiet {
            echo!("The schema is forward-compatible. Ready for upgrade.");
        }
        return Ok(());
    }
}

async fn single_check(ctx: &Context, cli: &mut Connection)
    -> anyhow::Result<CheckResult>
{
    use CheckResult::*;

    let bar = ProgressBar::new_spinner();
    bar.enable_steady_tick(Duration::from_millis(100));

    bar.set_message("checking schema");
    match execute_start_migration(ctx, cli).await {
        Ok(()) => {
            execute(cli, "ABORT MIGRATION").await?;
        }
        Err(e) if e.is::<EsdlError>() => {
            warn("Schema incompatibilities found. \
                  Please fix the errors above to proceed.");
            return Ok(SchemaIssue);
        }
        Err(e) => return Err(e),
    }

    bar.set_message("checking migrations");
    let migrations = migration::read_all(&ctx, true).await?;
    let old_timeout = timeout::inhibit_for_transaction(cli).await?;
    async_try! {
        async {
            execute(cli, "START MIGRATION REWRITE").await?;
            async_try! {
                async {
                    for migration in migrations.values() {
                        match apply_migration(cli, migration).await {
                            Ok(()) => {},
                            Err(e) if e.is::<ApplyMigrationError>() => {
                                bar.finish_and_clear();
                                print_apply_migration_error();
                                return Ok(MigrationsIssue);
                            }
                            Err(e) => return Err(e)?,
                        }
                    }
                    bar.finish_and_clear();
                    anyhow::Ok(Okay)
                },
                finally async {
                    execute_if_connected(cli, "ABORT MIGRATION REWRITE")
                        .await
                }
            }
        },
        finally async {
            timeout::restore_for_transaction(cli, old_timeout).await
        }
    }
}

fn print_apply_migration_error() {
    warn("The current schema is compatible. \
         But some of the migrations are outdated.");
    echo!("Please squash all migrations to fix the issue:");
    echo!("    edgedb migration create --squash".command_hint());
}

pub async fn watch_loop(mut rx: watch::Receiver<()>,
                        ctx: &Context, cli: &mut Connection, mut ok: bool)
    -> anyhow::Result<()>
{
    let mut retry_deadline = None::<Instant>;
    loop {
        // note we don't wait for interrupt here because if interrupt happens
        // the `background_for` method of the process takes care of it.
        cli.ping_while(wait_changes(&mut rx, retry_deadline)).await?;
        retry_deadline = None;
        match single_check(ctx, cli).await {
            Ok(CheckResult::Okay) => {
                if !ok {
                    success("The schema is forward-compatible. \
                            Ready for upgrade.");
                    ok = true;
                }
            }
            Ok(_) => {
                ok = false;
            }
            Err(e) => {
                ok = false;
                log::error!("Error updating database: {:#}. \
                             Will retry in 10 sec.", e);
                retry_deadline = Some(Instant::now() +
                                      Duration::from_secs(10));
            }
        }
    }
}
