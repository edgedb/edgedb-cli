use std::convert::TryInto;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use surf::http::auth::{AuthenticationScheme, Authorization};

use crate::commands::ExitCode;
use crate::options::CloudOptions;
use crate::platform::config_dir;
use crate::print;

const EDGEDB_CLOUD_BASE_URL: &str = "https://free-tier0.ovh-us-west-2.edgedb.cloud";
const EDGEDB_CLOUD_API_VERSION: &str = "/v1/";
const EDGEDB_CLOUD_API_TIMEOUT: u64 = 10;

#[derive(Debug, serde::Deserialize)]
struct ErrorResponse {
    status: String,
    error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CloudConfig {
    pub access_token: Option<String>,
}

pub struct CloudClient {
    client: surf::Client,
    pub is_logged_in: bool,
    pub base_url: String,
}

impl CloudClient {
    pub fn new(options: &CloudOptions) -> anyhow::Result<Self> {
        let access_token = if let Some(access_token) = &options.cloud_access_token {
            Some(access_token.into())
        } else {
            match fs::read_to_string(cloud_config_file()?) {
                Ok(data) if data.is_empty() => None,
                Ok(data) => {
                    let config: CloudConfig = serde_json::from_str(&data)?;
                    config.access_token
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(e)?;
                }
            }
        };
        let base_url = options
            .cloud_base_url
            .as_deref()
            .unwrap_or(
                env::var("EDGEDB_CLOUD_BASE_URL")
                    .as_deref()
                    .unwrap_or(EDGEDB_CLOUD_BASE_URL),
            )
            .to_string();
        let mut config = surf::Config::new()
            .set_base_url(surf::Url::parse(&base_url)?.join(EDGEDB_CLOUD_API_VERSION)?)
            .set_timeout(Some(Duration::from_secs(EDGEDB_CLOUD_API_TIMEOUT)));
        let is_logged_in = if let Some(access_token) = access_token {
            let auth = Authorization::new(AuthenticationScheme::Bearer, access_token.into());
            config = config
                .add_header(auth.name(), auth.value())
                .map_err(HttpError)?;
            true
        } else {
            false
        };
        Ok(Self {
            client: config.try_into()?,
            is_logged_in,
            base_url,
        })
    }

    pub fn ensure_authenticated(&self, quiet: bool) -> anyhow::Result<()> {
        if self.is_logged_in {
            Ok(())
        } else {
            if !quiet {
                print::error("Run `edgedb cloud login` first.");
            }
            Err(ExitCode::new(9).into())
        }
    }

    pub async fn request<T: serde::de::DeserializeOwned>(
        &self,
        req: surf::RequestBuilder,
    ) -> anyhow::Result<T> {
        let mut resp = req.await.map_err(HttpError)?;
        if !resp.status().is_success() {
            let ErrorResponse { status, error } = resp.body_json().await.map_err(HttpError)?;
            if let Some(error) = error {
                anyhow::bail!(format!(
                    "Failed to create authentication session: {}: {}",
                    status, error
                ));
            } else {
                anyhow::bail!(format!(
                    "Failed to create authentication session: {}",
                    status
                ));
            }
        }
        Ok(resp.body_json().await.map_err(HttpError)?)
    }

    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
    ) -> anyhow::Result<T> {
        self.request(self.client.get(uri)).await
    }

    pub async fn post<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
        body: impl Into<surf::Body>,
    ) -> anyhow::Result<T> {
        self.request(self.client.post(uri).body(body)).await
    }

    pub async fn put<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
        body: impl Into<surf::Body>,
    ) -> anyhow::Result<T> {
        self.request(self.client.put(uri).body(body)).await
    }

    pub async fn delete<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
    ) -> anyhow::Result<T> {
        self.request(self.client.delete(uri)).await
    }
}

pub fn cloud_config_file() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("cloud.json"))
}
