use std::fs;
use std::io;
use std::path::PathBuf;

use async_std::task;
use colorful::Colorful;
use edgedb_client::credentials::Credentials;
use edgedb_client::Builder;

use crate::cloud::auth;
use crate::credentials;
use crate::options::CloudOptions;
use crate::print::{self, Highlight};
use crate::question;

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, serde::Deserialize)]
struct CloudInstance {
    id: String,
    name: String,
    dsn: String,
}

#[derive(Debug, serde::Serialize)]
struct CloudInstanceQuery {
    name: String,
}

#[derive(Debug, serde::Serialize)]
struct CloudInstanceCreate {
    name: String,
    nightly: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_database: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_user: Option<String>,
}

pub async fn create(
    cmd: &crate::portable::options::Create,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let options = &opts.cloud_options;
    let base_url = auth::get_base_url(options);
    let access_token = if let Some(access_token) = auth::get_access_token(options)? {
        access_token
    } else {
        anyhow::bail!("Run `edgedb cloud login` first.");
    };
    let cred_path = credentials::path(&cmd.name)?;
    if cred_path.exists() {
        anyhow::bail!("File {} exists; abort.", cred_path.display());
    }
    let mut resp = surf::post(format!("{}/v1/edgedb-instances/", base_url))
        .body(serde_json::to_value(CloudInstanceCreate {
            name: cmd.name.clone(),
            nightly: cmd.nightly,
            version: cmd.version.clone().map(|o| format!("{}", o)),
            default_database: Some(cmd.default_database.clone()),
            default_user: Some(cmd.default_user.clone()),
        })?)
        .header("Authorization", format!("Bearer {}", access_token))
        .await
        .map_err(HttpError)?;
    auth::raise_http_error(&mut resp).await?;
    let CloudInstance { id, dsn, name: _ } = resp.body_json().await.map_err(HttpError)?;
    let mut creds = Builder::uninitialized()
        .read_dsn(&dsn)
        .await?
        .as_credentials()?;
    creds.cloud_instance_id = Some(id);
    creds.cloud_original_dsn = Some(dsn);
    credentials::write(&cred_path, &creds).await?;
    print::echo!(
        "EdgeDB Cloud instance",
        cmd.name.emphasize(),
        "is up and running."
    );
    print::echo!("To connect to the instance run:");
    print::echo!("  edgedb -I", cmd.name);
    Ok(())
}

pub async fn link(
    cmd: &crate::portable::options::Link,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    if cmd.non_interactive {
        if let Some(name) = &cmd.name {
            if !crate::portable::local::is_valid_name(name) {
                print::error(
                    "Instance name must be a valid identifier, \
                             (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                );
            }
            let cred_path = credentials::path(name)?;
            if cred_path.exists() && !cmd.overwrite {
                anyhow::bail!("File {} exists; abort.", cred_path.display());
            }
        } else {
            anyhow::bail!("Name is mandatory if --non-interactive is set.");
        }
    }
    let options = &opts.cloud_options;
    let base_url = auth::get_base_url(options);
    let access_token = if let Some(access_token) = auth::get_access_token(options)? {
        access_token
    } else {
        if cmd.non_interactive {
            anyhow::bail!("Run `edgedb cloud login` first.");
        } else {
            let q = question::Confirm::new(
                "You're not authenticated to the EdgeDB Cloud yet, login now?",
            );
            if q.ask()? {
                auth::login(&crate::cloud::options::Login {}, options).await?;
                if let Some(access_token) = auth::get_access_token(options)? {
                    access_token
                } else {
                    anyhow::bail!("Couldn't fetch access token.");
                }
            } else {
                print::error("Aborted.");
                return Ok(());
            }
        }
    };
    let cloud_name = if let Some(name) = &cmd.name {
        name.clone()
    } else {
        if cmd.non_interactive {
            unreachable!("Already checked previously");
        } else {
            loop {
                let name = question::String::new(
                    "Input the name of the EdgeDB Cloud instance to connect to",
                )
                .ask()?;
                if !crate::portable::local::is_valid_name(&name) {
                    print::error(
                        "Instance name must be a valid identifier, \
                                 (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                    );
                    continue;
                }
                break name;
            }
        }
    };
    let mut resp = surf::get(format!("{}/v1/edgedb-instances/", base_url))
        .query(&CloudInstanceQuery {
            name: cloud_name.clone(),
        })
        .map_err(HttpError)?
        .header("Authorization", format!("Bearer {}", access_token))
        .await
        .map_err(HttpError)?;
    auth::raise_http_error(&mut resp).await?;
    let CloudInstance { id, dsn, name: _ } = resp.body_json().await.map_err(HttpError)?;

    let (cred_path, instance_name) = if let Some(name) = &cmd.name {
        let cred_path = credentials::path(&name)?;
        if cred_path.exists() && cmd.overwrite && !cmd.quiet {
            print::warn(format!("Overwriting {}", cred_path.display()));
        }
        (cred_path, name.clone())
    } else {
        assert!(!cmd.non_interactive, "Already checked previously");
        let same_name_exists = credentials::path(&cloud_name)?.exists() && !cmd.overwrite;
        loop {
            let name = if same_name_exists {
                question::String::new("Specify a local name to refer to the EdgeDB Cloud instance")
                    .ask()?
            } else {
                question::String::new("Use the same name locally?")
                    .default(&cloud_name)
                    .ask()?
            };
            if !crate::portable::local::is_valid_name(&name) {
                print::error(
                    "Instance name must be a valid identifier, \
                         (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                );
                continue;
            }
            let cred_path = credentials::path(&name)?;
            if cred_path.exists() {
                if cmd.overwrite {
                    if !cmd.quiet {
                        print::warn(format!("Overwriting {}", cred_path.display()));
                    }
                } else {
                    let mut q = question::Confirm::new_dangerous(format!(
                        "{} exists! Overwrite?",
                        cred_path.display()
                    ));
                    q.default(false);
                    if !q.ask()? {
                        continue;
                    }
                }
            }
            break (cred_path, name);
        }
    };

    let mut creds = Builder::uninitialized()
        .read_dsn(&dsn)
        .await?
        .as_credentials()?;
    creds.cloud_instance_id = Some(id);
    creds.cloud_original_dsn = Some(dsn);
    credentials::write(&cred_path, &creds).await?;
    if !cmd.quiet {
        let mut msg = "Successfully linked to EdgeDB Cloud instance.".to_string();
        if print::use_color() {
            msg = format!("{}", msg.bold().light_green());
        }
        eprintln!(
            "{} To connect run:\
            \n  edgedb -I {}",
            msg,
            instance_name.escape_default(),
        );
    }
    Ok(())
}

async fn destroy(instance_id: &str, options: &CloudOptions) -> anyhow::Result<()> {
    log::info!("Destroying EdgeDB Cloud instance: {}", instance_id);
    let base_url = auth::get_base_url(options);
    let access_token = if let Some(token) = auth::get_access_token(options)? {
        token
    } else {
        anyhow::bail!("Cloud authentication required.");
    };
    let mut resp = surf::delete(format!("{}/v1/edgedb-instances/{}", base_url, instance_id))
        .header("Authorization", format!("Bearer {}", access_token))
        .await
        .map_err(HttpError)?;
    auth::raise_http_error(&mut resp).await?;
    Ok(())
}

pub fn try_to_destroy(
    cred_path: &PathBuf,
    options: &crate::options::Options,
) -> anyhow::Result<()> {
    let file = io::BufReader::new(fs::File::open(cred_path)?);
    let credentials: Credentials = serde_json::from_reader(file)?;
    if let Some(instance_id) = credentials.cloud_instance_id {
        task::block_on(destroy(&instance_id, &options.cloud_options))?
    }
    Ok(())
}
