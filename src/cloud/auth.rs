use std::time::{Duration, Instant};

use tokio::time::sleep;

use crate::cloud::client::{cloud_config_file, CloudClient, CloudConfig};
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

#[tokio::main]
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
                token: Some(token),
            }) => {
                write_json(
                    &cloud_config_file()?,
                    "cloud config",
                    &CloudConfig {
                        access_token: Some(token),
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
        sleep(AUTHENTICATION_POLL_INTERVAL).await;
    }
    anyhow::bail!(
        "Authentication is expected to be done in {:?}.",
        AUTHENTICATION_WAIT_TIME
    )
}

#[tokio::main]
pub async fn logout(_c: &options::Logout) -> anyhow::Result<()> {
    write_json(
        &cloud_config_file()?,
        "cloud config",
        &CloudConfig { access_token: None },
    )?;
    print::success("You're now logged out from EdgeDB Cloud.");
    Ok(())
}
