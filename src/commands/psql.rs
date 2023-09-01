use std::env;
use std::process::Command;
use std::ffi::OsString;

use anyhow::Context;
use crate::connect::Connection;
use edgedb_tokio::server_params::PostgresAddress;

use crate::commands::Options;
use crate::print;
use crate::interrupt;


pub async fn psql<'x>(cli: &mut Connection, _options: &Options)
    -> Result<(), anyhow::Error>
{
    match cli.get_server_param::<PostgresAddress>() {
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


            let _trap = interrupt::Trap::trap(&[interrupt::Signal::Interrupt]);
            cmd.status().with_context(|| {
                format!("Error running {:?} (path: {:?})", cmd,
                        path.unwrap_or_else(OsString::new))
            })?;
        }
        None => {
            print::error("EdgeDB must be run in DEV mode to use psql.");
        }
    }
    Ok(())
}
