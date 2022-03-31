use std::time::Duration;

use async_std::task;

use crate::cloud::client::{cloud_config_file, CloudClient, CloudConfig};
use crate::cloud::options;
use crate::options::CloudOptions;
use crate::portable::local::write_json;
use crate::print;

const AUTHENTICATION_WAIT_TIME: i32 = 180;

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
    let link = format!("{}{}", client.base_url, auth_url);
    log::debug!("Opening URL in browser: {}", link);
    if open::that(&link).is_ok() {
        print::prompt("Please complete the authentication in the opened browser.");
    } else {
        print::prompt("Please open this link in your browser and complete the authentication:");
        print::success_msg("Link", link);
    }
    for _ in 0..AUTHENTICATION_WAIT_TIME {
        let UserSession {
            id: _,
            auth_url: _,
            token,
        } = client.get(format!("auth/sessions/{}", id)).await?;
        if let Some(token) = token {
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
        task::sleep(Duration::from_secs(1)).await;
    }
    anyhow::bail!("Timed out.")
}

pub async fn logout(_c: &options::Logout) -> anyhow::Result<()> {
    write_json(
        &cloud_config_file()?,
        "cloud config",
        &CloudConfig { access_token: None },
    )?;
    print::success("You're now logged out from EdgeDB Cloud.");
    Ok(())
}
