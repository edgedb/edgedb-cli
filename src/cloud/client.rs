use std::convert::TryInto;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use surf::http::auth::{AuthenticationScheme, Authorization};

use crate::options::CloudOptions;
use crate::platform::config_dir;

const EDGEDB_CLOUD_DEFAULT_DNS_ZONE: &str = "aws.edgedb.cloud";
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

#[derive(Debug, serde::Deserialize)]
struct Claims {
    #[serde(rename = "iss", skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
}

pub struct CloudClient {
    client: surf::Client,
    pub is_logged_in: bool,
    pub api_endpoint: String,
    options_access_token: Option<String>,
    options_api_endpoint: Option<String>,
    dns_zone: String,
    pub access_token: Option<String>,
}

impl CloudClient {
    pub fn new(options: &CloudOptions) -> anyhow::Result<Self> {
        Self::new_inner(&options.cloud_access_token, &options.cloud_api_endpoint)
    }

    fn new_inner(
        options_access_token: &Option<String>, options_api_endpoint: &Option<String>
    ) -> anyhow::Result<Self> {
        let access_token = if let Some(access_token) = options_access_token {
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
        let mut config = surf::Config::new()
            .set_timeout(Some(Duration::from_secs(EDGEDB_CLOUD_API_TIMEOUT)));
        let is_logged_in;
        let dns_zone;
        if let Some(access_token) = access_token.clone() {
            let claims_b64 = access_token
                .splitn(3, ".")
                .skip(1)
                .next()
                .context("Illegal JWT token")?;
            let claims = base64::decode_config(claims_b64, base64::URL_SAFE_NO_PAD)?;
            let claims: Claims = serde_json::from_slice(&claims)?;
            dns_zone = claims.issuer;

            let auth = Authorization::new(AuthenticationScheme::Bearer, access_token);
            config = config
                .add_header(auth.name(), auth.value())
                .map_err(HttpError)?;
            is_logged_in = true;
        } else {
            dns_zone = None;
            is_logged_in = false;
        }
        let dns_zone = dns_zone.unwrap_or_else(|| EDGEDB_CLOUD_DEFAULT_DNS_ZONE.to_string());
        let api_endpoint = options_api_endpoint
            .clone()
            .or_else(|| env::var("EDGEDB_CLOUD_API_ENDPOINT").ok())
            .unwrap_or_else(|| format!("https://api.g.{dns_zone}"));
        config = config
            .set_base_url(surf::Url::parse(&api_endpoint)?.join(EDGEDB_CLOUD_API_VERSION)?);
        Ok(Self {
            client: config.try_into()?,
            is_logged_in,
            api_endpoint,
            options_access_token: options_access_token.clone(),
            options_api_endpoint: options_api_endpoint.clone(),
            dns_zone,
            access_token,
        })
    }

    pub fn reinit(&mut self) -> anyhow::Result<()> {
        *self = Self::new_inner(
            &self.options_access_token,
            &self.options_api_endpoint,
        )?;
        Ok(())
    }

    pub fn ensure_authenticated(&self) -> anyhow::Result<()> {
        if self.is_logged_in {
            Ok(())
        } else {
            anyhow::bail!("Run `edgedb cloud login` first.")
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

    pub fn get_cloud_host(&self, org: &str, inst: &str) -> String {
        let msg = format!("{}/{}", org, inst);
        let checksum = crc16::State::<crc16::XMODEM>::calculate(msg.as_bytes());
        let dns_bucket = format!("c-{:x}", checksum % 9900);
        format!("{}.{}.{}.i.{}", inst, org, dns_bucket, self.dns_zone)
    }
}

pub fn cloud_config_file() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("cloud.json"))
}
