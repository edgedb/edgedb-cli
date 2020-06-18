use std::env;
use std::process::Command;
use std::ffi::OsString;

use anyhow::Context;
use crate::client::Connection;
use crate::commands::Options;
use crate::server_params::PostgresAddress;


pub async fn psql<'x>(cli: &mut Connection, _options: &Options)
    -> Result<(), anyhow::Error>
{
    match cli.get_param::<PostgresAddress>() {
        Some(addr) => {
            let mut cmd = Command::new("psql");
            let path = if cfg!(feature="dev_mode") {
                use std::iter;
                use std::path::{Path, PathBuf};

                if let Some(dir) = option_env!("PSQL_DEFAULT_PATH") {
                    let psql_path = Path::new(dir).join("psql");
                    if !psql_path.exists() {
                        eprintln!("WARNING: {} does not exist",
                                  psql_path.display());
                    }
                    let npath = if let Some(path) = env::var_os("PATH") {
                        env::join_paths(
                            iter::once(PathBuf::from(dir))
                            .chain(env::split_paths(&path)))
                        .unwrap_or_else(|e| {
                            eprintln!("PSQL_DEFAULT_PATH error: {}", e);
                            path
                        })
                    } else {
                        dir.into()
                    };
                    Some(npath)
                } else {
                    env::var_os("PATH")
                }
            } else {
                env::var_os("PATH")
            };
            cmd.arg("-h").arg(&addr.host);
            cmd.arg("-U").arg(&addr.user);
            cmd.arg("-p").arg(addr.port.to_string());
            cmd.arg("-d").arg(&addr.database);
            if let Some(path) = path.as_ref() {
                cmd.env("PATH", path);
            }

            #[cfg(unix)]
            let _trap = signal::trap::Trap::trap(&[signal::Signal::SIGINT]);
            cmd.status().context(
                format!("Error running {:?} (path: {:?})", cmd,
                    path.unwrap_or_else(OsString::new)))?;
        }
        None => {
            eprintln!("psql requires EdgeDB to run in DEV mode");
        }
    }
    Ok(())
}
