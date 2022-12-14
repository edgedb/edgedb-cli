use std::io;
use std::time::{Duration, Instant};

use anyhow::Context;
use async_std::task;
use fs_err as fs;

use crate::cloud::client::{cloud_config_dir, cloud_config_file, CloudClient, CloudConfig};
use crate::cloud::options;
use crate::options::CloudOptions;
use crate::portable::local::write_json;
use crate::print;

const AUTHENTICATION_WAIT_TIME: Duration = Duration::from_secs(10 * 60);
const AUTHENTICATION_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, serde::Deserialize)]
struct UserSession {
    id: String,
    token: Option<String>,
    auth_url: String,
}

pub async fn login(_c: &options::Login, options: &CloudOptions) -> anyhow::Result<()> {
    do_login(&CloudClient::new(options)?).await
}

pub async fn do_login(client: &CloudClient) -> anyhow::Result<()> {
    let UserSession {
        id,
        auth_url,
        token: _,
    } = client
        .post("auth/sessions/", serde_json::json!({ "type": "CLI" }))
        .await?;
    let link = format!("{}{}", client.api_endpoint, auth_url);
    log::debug!("Opening URL in browser: {}", link);
    if open::that(&link).is_ok() {
        print::prompt("Please complete the authentication in the opened browser.");
    } else {
        print::prompt("Please open this link in your browser and complete the authentication:");
        print::success_msg("Link", link);
    }
    let deadline = Instant::now() + AUTHENTICATION_WAIT_TIME;
    while Instant::now() < deadline {
        match client.get(format!("auth/sessions/{}", id)).await {
            Ok(UserSession {
                id: _,
                auth_url: _,
                token: Some(secret_key),
            }) => {
                write_json(
                    &cloud_config_file(&client.profile)?,
                    "cloud config",
                    &CloudConfig {
                        secret_key: Some(secret_key),
                    },
                )?;
                print::success("Successfully authenticated to EdgeDB Cloud.");
                return Ok(());
            }
            Err(e) => print::warn(format!(
                "Retrying to get results because request failed: {:?}",
                e
            )),
            _ => {}
        }
        task::sleep(AUTHENTICATION_POLL_INTERVAL).await;
    }
    anyhow::bail!(
        "Authentication is expected to be done in {:?}.",
        AUTHENTICATION_WAIT_TIME
    )
}

pub fn logout(c: &options::Logout, options: &CloudOptions) -> anyhow::Result<()> {
    if c.all_profiles {
        let cloud_creds = cloud_config_dir()?;
        let dir_entries = match fs::read_dir(cloud_creds.clone()) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => anyhow::bail!(e),
        };
        for item in dir_entries {
            let item = item?;
            fs::remove_file(cloud_creds.join(item.file_name()))?;
        }
        print::success("You're now logged out from EdgeDB Cloud.");
    } else {
        let client = CloudClient::new(options)?;
        let path = cloud_config_file(&client.profile)?;
        if path.exists() {
            fs::remove_file(path).with_context(|| "failed to logout")?;
            print::success(format!(
                "You're now logged out from EdgeDB Cloud for profile \"{}\".",
                client.profile.as_deref().unwrap_or("default")
            ));
        } else {
            print::warn(format!(
                "You're already logged out from EdgeDB Cloud for profile \"{}\".",
                client.profile.as_deref().unwrap_or("default")
            ));
        }
    }
    Ok(())
}
