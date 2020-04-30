use std::env;
use std::path::Path;
use std::ffi::OsString;
use std::process::Command;

use anyhow::Context;
use crate::client::Client;
use crate::commands::Options;
use crate::server_params::PostgresAddress;


pub async fn psql<'x>(cli: &mut Client<'x>, _options: &Options)
    -> Result<(), anyhow::Error>
{
    match cli.params.get::<PostgresAddress>() {
        Some(addr) => {
            let mut cmd = Command::new("psql");
            #[cfg(all(feature="dev_mode", unix))]
            {
                if let Some(dir) = option_env!("PSQL_DEFAULT_PATH") {
                    let psql_path = Path::new(dir).join("psql");
                    if !psql_path.exists() {
                        eprintln!("WARNING: {} does not exists",
                                  psql_path.display());
                    }
                    let npath = if let Some(path) = env::var_os("PATH") {
                        if path.is_empty() {
                            dir.into()
                        } else {
                            let mut npath = OsString::with_capacity(
                                dir.len() + 1 + path.len() + 1);
                            npath.push(dir);
                            npath.push(":");
                            npath.push(path);
                            npath
                        }
                    } else {
                        dir.into()
                    };
                    cmd.env("PATH", npath);
                }
            }
            cmd.arg("-h").arg(&addr.host);
            cmd.arg("-U").arg(&addr.user);
            cmd.arg("-p").arg(addr.port.to_string());
            cmd.arg("-d").arg(&addr.database);
            cmd.status()
                .context(format!("Error running {:?}", cmd))?;
        }
        None => {
            eprintln!("psql requires EdgeDB to run in DEV mode");
        }
    }
    Ok(())
}
