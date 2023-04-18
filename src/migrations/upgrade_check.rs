use std::path::Path;

use anyhow::Context as _;
use edgedb_tokio::Builder;
use tokio::fs;

use crate::async_try;
use crate::commands::{Options, ExitCode};
use crate::connect::Connection;
use crate::migrations::context::Context;
use crate::migrations::create::{execute_start_migration, EsdlError};
use crate::migrations::edb::{execute, execute_if_connected};
use crate::migrations::migration;
use crate::migrations::options::UpgradeCheck;
use crate::migrations::migrate::{apply_migration, ApplyMigrationError};
use crate::portable::install;
use crate::portable::repository::{self, Query};
use crate::print::{warn, echo, Highlight};
use crate::process;
use crate::migrations::timeout;

#[derive(Debug, serde::Deserialize)]
struct EdgedbStatus {
    port: u16,
    tls_cert_file: String,
}


pub fn upgrade_check(_options: &Options, options: &UpgradeCheck)
    -> anyhow::Result<()>
{
    if options.watch {
        anyhow::bail!("watch mode is not implemented yet");
    }
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

                do_check(&ctx, &status_dir.path().join("status")).await
            })
        })
    }
}


async fn do_check(ctx: &Context, status_file: &Path) -> anyhow::Result<()> {
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

    match execute_start_migration(ctx, cli).await {
        Ok(()) => {
            execute(cli, "ABORT MIGRATION").await?;
        }
        Err(e) if e.is::<EsdlError>() => {
            warn("Schema incompatibilities found. \
                  Please fix the errors above to proceed.");
            echo!("For faster feedback loop use:");
            echo!("    edgedb migration upgrade-check --watch".command_hint());
            return Err(ExitCode::new(3))?;
        }
        Err(e) => return Err(e),
    }

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
                                print_apply_migration_error();
                                return Err(ExitCode::new(4))?;
                            }
                            Err(e) => return Err(e)?,
                        }
                    }
                    anyhow::Ok(())
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
    }?;
    if !ctx.quiet {
        echo!("The schema is forward-compatible. Ready for upgrade.");
    }
    Ok(())
}

fn print_apply_migration_error() {
    warn("The current schema is compatible. \
         But some of the migrations are outdated.");
    echo!("Please squash all migrations to fix the issue:");
    echo!("    edgedb migration create --squash".command_hint());
}
