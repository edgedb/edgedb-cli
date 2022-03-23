use std::fs;
use std::io;
use std::time::Duration;

use async_std::task;

use crate::cloud::options;
use crate::options::CloudOptions;
use crate::platform::config_dir;
use crate::portable::local::write_json;
use crate::print;
use std::path::PathBuf;

const EDGEDB_CLOUD_BASE_URL: &str = "http://127.0.0.1:5959";
const AUTHENTICATION_WAIT_TIME: i32 = 180;

#[derive(Debug, serde::Deserialize)]
struct ErrorResponse {
    status: String,
    error: String,
}

#[derive(Debug, serde::Deserialize)]
struct UserSession {
    id: String,
    token: Option<String>,
    auth_url: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CloudConfig {
    access_token: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

fn cloud_config_file() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("cloud.json"))
}

pub async fn login(_c: &options::Login, options: &CloudOptions) -> anyhow::Result<()> {
    let base_url = options
        .cloud_base_url
        .as_deref()
        .unwrap_or(EDGEDB_CLOUD_BASE_URL);
    let mut resp = surf::post(format!("{}/v1/auth/sessions/", base_url))
        .content_type(surf::http::mime::JSON)
        .body("{\"type\":\"CLI\"}")
        .await
        .map_err(HttpError)?;
    if !resp.status().is_success() {
        let ErrorResponse { status, error } = resp.body_json().await.map_err(HttpError)?;
        anyhow::bail!(format!(
            "Failed to create authentication session: {}: {}",
            status, error
        ));
    }
    let UserSession {
        id,
        auth_url,
        token: _,
    } = resp.body_json().await.map_err(HttpError)?;
    let link = format!("{}{}", base_url, auth_url);
    log::debug!("Opening URL in browser: {}", link);
    if open::that(&link).is_ok() {
        print::prompt("Please complete the authentication in the opened browser.");
    } else {
        print::prompt("Please open this link in your browser and complete the authentication:");
        print::success_msg("Link", link);
    }
    for _ in 0..AUTHENTICATION_WAIT_TIME {
        resp = surf::get(format!("{}/v1/auth/sessions/{}", base_url, id))
            .await
            .map_err(HttpError)?;
        let UserSession {
            id: _,
            auth_url: _,
            token,
        } = resp.body_json().await.map_err(HttpError)?;
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
    print::error("Timed out.");
    Ok(())
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

pub fn get_access_token(options: &CloudOptions) -> anyhow::Result<Option<String>> {
    if let Some(access_token) = &options.cloud_access_token {
        return Ok(Some(access_token.clone()));
    }
    let data = match fs::read_to_string(cloud_config_file()?) {
        Ok(data) if data.is_empty() => {
            return Ok(None);
        }
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => {
            return Err(e)?;
        }
    };
    let config: CloudConfig = serde_json::from_str(&data)?;
    Ok(config.access_token)
}
