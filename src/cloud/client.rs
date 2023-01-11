use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use reqwest::header;

use crate::options::CloudOptions;
use crate::platform::config_dir;

const EDGEDB_CLOUD_DEFAULT_DNS_ZONE: &str = "aws.edgedb.cloud";
const EDGEDB_CLOUD_API_VERSION: &str = "v1/";
const EDGEDB_CLOUD_API_TIMEOUT: u64 = 10;

#[derive(Debug, serde::Deserialize)]
struct ErrorResponse {
    status: String,
    error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(reqwest::Error);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CloudConfig {
    pub secret_key: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Claims {
    #[serde(rename = "iss", skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
}

pub struct CloudClient {
    client: reqwest::Client,
    pub is_logged_in: bool,
    pub api_endpoint: reqwest::Url,
    options_secret_key: Option<String>,
    options_profile: Option<String>,
    options_api_endpoint: Option<String>,
    pub secret_key: Option<String>,
    pub profile: Option<String>,
}

impl CloudClient {
    pub fn new(options: &CloudOptions) -> anyhow::Result<Self> {
        Self::new_inner(
            &options.cloud_secret_key,
            &options.cloud_profile,
            &options.cloud_api_endpoint,
        )
    }

    fn new_inner(
        options_secret_key: &Option<String>,
        options_profile: &Option<String>,
        options_api_endpoint: &Option<String>,
    ) -> anyhow::Result<Self> {
        let profile = options_profile
            .clone()
            .or_else(|| env::var("EDGEDB_CLOUD_PROFILE").ok());
        let secret_key = if let Some(secret_key) = options_secret_key {
            Some(secret_key.into())
        } else if let Ok(secret_key) = env::var("EDGEDB_CLOUD_SECRET_KEY") {
            Some(secret_key)
        } else if let Ok(secret_key) = env::var("EDGEDB_SECRET_KEY") {
            Some(secret_key)
        } else {
            match fs::read_to_string(cloud_config_file(&profile)?) {
                Ok(data) if data.is_empty() => None,
                Ok(data) => {
                    let config: CloudConfig = serde_json::from_str(&data)?;
                    config.secret_key
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(e)?;
                }
            }
        };
        let mut builder =
            reqwest::Client::builder().timeout(Duration::from_secs(EDGEDB_CLOUD_API_TIMEOUT));
        let is_logged_in;
        let dns_zone;
        if let Some(secret_key) = secret_key.clone() {
            let claims_b64 = secret_key
                .splitn(3, ".")
                .skip(1)
                .next()
                .context("Illegal JWT token")?;
            let claims = base64::decode_config(claims_b64, base64::URL_SAFE_NO_PAD)?;
            let claims: Claims = serde_json::from_slice(&claims)?;
            dns_zone = claims.issuer;

            let mut headers = header::HeaderMap::new();
            let auth_str = format!("Bearer {}", secret_key);
            let mut auth_value = header::HeaderValue::from_str(&auth_str)?;
            auth_value.set_sensitive(true);
            headers.insert(header::AUTHORIZATION, auth_value);
            builder = builder.default_headers(headers);
            is_logged_in = true;
        } else {
            dns_zone = None;
            is_logged_in = false;
        }
        let dns_zone = dns_zone.unwrap_or_else(|| EDGEDB_CLOUD_DEFAULT_DNS_ZONE.to_string());
        let api_endpoint = options_api_endpoint
            .clone()
            .map(Ok)
            .or_else(|| env::var_os("EDGEDB_CLOUD_API_ENDPOINT").map(|v| v.into_string()))
            .transpose()
            .map_err(|v| anyhow::anyhow!("cannot decode EDGEDB_CLOUD_API_ENDPOINT: {:?}", v))?
            .or_else(|| Some(format!("https://api.g.{dns_zone}")))
            .as_deref()
            .map(reqwest::Url::parse)
            .unwrap()
            .and_then(|u| u.join(EDGEDB_CLOUD_API_VERSION))?;
        let cloud_certs = env::var_os("_EDGEDB_CLOUD_CERTS")
            .map(|v| v.into_string())
            .transpose()
            .map_err(|v| anyhow::anyhow!("cannot decode _EDGEDB_CLOUD_CERTS: {:?}", v))?;
        if matches!(cloud_certs.as_deref(), Some("staging")) {
            builder = builder
                .add_root_certificate(
                    reqwest::Certificate::from_pem(
                        "-----BEGIN CERTIFICATE-----
MIIFmDCCA4CgAwIBAgIQU9C87nMpOIFKYpfvOHFHFDANBgkqhkiG9w0BAQsFADBm
MQswCQYDVQQGEwJVUzEzMDEGA1UEChMqKFNUQUdJTkcpIEludGVybmV0IFNlY3Vy
aXR5IFJlc2VhcmNoIEdyb3VwMSIwIAYDVQQDExkoU1RBR0lORykgUHJldGVuZCBQ
ZWFyIFgxMB4XDTE1MDYwNDExMDQzOFoXDTM1MDYwNDExMDQzOFowZjELMAkGA1UE
BhMCVVMxMzAxBgNVBAoTKihTVEFHSU5HKSBJbnRlcm5ldCBTZWN1cml0eSBSZXNl
YXJjaCBHcm91cDEiMCAGA1UEAxMZKFNUQUdJTkcpIFByZXRlbmQgUGVhciBYMTCC
AiIwDQYJKoZIhvcNAQEBBQADggIPADCCAgoCggIBALbagEdDTa1QgGBWSYkyMhsc
ZXENOBaVRTMX1hceJENgsL0Ma49D3MilI4KS38mtkmdF6cPWnL++fgehT0FbRHZg
jOEr8UAN4jH6omjrbTD++VZneTsMVaGamQmDdFl5g1gYaigkkmx8OiCO68a4QXg4
wSyn6iDipKP8utsE+x1E28SA75HOYqpdrk4HGxuULvlr03wZGTIf/oRt2/c+dYmD
oaJhge+GOrLAEQByO7+8+vzOwpNAPEx6LW+crEEZ7eBXih6VP19sTGy3yfqK5tPt
TdXXCOQMKAp+gCj/VByhmIr+0iNDC540gtvV303WpcbwnkkLYC0Ft2cYUyHtkstO
fRcRO+K2cZozoSwVPyB8/J9RpcRK3jgnX9lujfwA/pAbP0J2UPQFxmWFRQnFjaq6
rkqbNEBgLy+kFL1NEsRbvFbKrRi5bYy2lNms2NJPZvdNQbT/2dBZKmJqxHkxCuOQ
FjhJQNeO+Njm1Z1iATS/3rts2yZlqXKsxQUzN6vNbD8KnXRMEeOXUYvbV4lqfCf8
mS14WEbSiMy87GB5S9ucSV1XUrlTG5UGcMSZOBcEUpisRPEmQWUOTWIoDQ5FOia/
GI+Ki523r2ruEmbmG37EBSBXdxIdndqrjy+QVAmCebyDx9eVEGOIpn26bW5LKeru
mJxa/CFBaKi4bRvmdJRLAgMBAAGjQjBAMA4GA1UdDwEB/wQEAwIBBjAPBgNVHRMB
Af8EBTADAQH/MB0GA1UdDgQWBBS182Xy/rAKkh/7PH3zRKCsYyXDFDANBgkqhkiG
9w0BAQsFAAOCAgEAncDZNytDbrrVe68UT6py1lfF2h6Tm2p8ro42i87WWyP2LK8Y
nLHC0hvNfWeWmjZQYBQfGC5c7aQRezak+tHLdmrNKHkn5kn+9E9LCjCaEsyIIn2j
qdHlAkepu/C3KnNtVx5tW07e5bvIjJScwkCDbP3akWQixPpRFAsnP+ULx7k0aO1x
qAeaAhQ2rgo1F58hcflgqKTXnpPM02intVfiVVkX5GXpJjK5EoQtLceyGOrkxlM/
sTPq4UrnypmsqSagWV3HcUlYtDinc+nukFk6eR4XkzXBbwKajl0YjztfrCIHOn5Q
CJL6TERVDbM/aAPly8kJ1sWGLuvvWYzMYgLzDul//rUF10gEMWaXVZV51KpS9DY/
5CunuvCXmEQJHo7kGcViT7sETn6Jz9KOhvYcXkJ7po6d93A/jy4GKPIPnsKKNEmR
xUuXY4xRdh45tMJnLTUDdC9FIU0flTeO9/vNpVA8OPU1i14vCz+MU8KX1bV3GXm/
fxlB7VBBjX9v5oUep0o/j68R/iDlCOM4VVfRa8gX6T2FU7fNdatvGro7uQzIvWof
gN9WUwCbEMBy/YhBSrXycKA8crgGg3x1mIsopn88JKwmMBa68oS7EHM9w7C4y71M
7DiA+/9Qdp9RBWJpTS9i/mDnJg1xvo8Xz49mrrgfmcAXTCJqXi24NatI3Oc=
-----END CERTIFICATE-----"
                        .as_bytes(),
                    )
                    .unwrap(),
                )
                .add_root_certificate(
                    reqwest::Certificate::from_pem(
                        "-----BEGIN CERTIFICATE-----
MIICTjCCAdSgAwIBAgIRAIPgc3k5LlLVLtUUvs4K/QcwCgYIKoZIzj0EAwMwaDEL
MAkGA1UEBhMCVVMxMzAxBgNVBAoTKihTVEFHSU5HKSBJbnRlcm5ldCBTZWN1cml0
eSBSZXNlYXJjaCBHcm91cDEkMCIGA1UEAxMbKFNUQUdJTkcpIEJvZ3VzIEJyb2Nj
b2xpIFgyMB4XDTIwMDkwNDAwMDAwMFoXDTQwMDkxNzE2MDAwMFowaDELMAkGA1UE
BhMCVVMxMzAxBgNVBAoTKihTVEFHSU5HKSBJbnRlcm5ldCBTZWN1cml0eSBSZXNl
YXJjaCBHcm91cDEkMCIGA1UEAxMbKFNUQUdJTkcpIEJvZ3VzIEJyb2Njb2xpIFgy
MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAEOvS+w1kCzAxYOJbA06Aw0HFP2tLBLKPo
FQqR9AMskl1nC2975eQqycR+ACvYelA8rfwFXObMHYXJ23XLB+dAjPJVOJ2OcsjT
VqO4dcDWu+rQ2VILdnJRYypnV1MMThVxo0IwQDAOBgNVHQ8BAf8EBAMCAQYwDwYD
VR0TAQH/BAUwAwEB/zAdBgNVHQ4EFgQU3tGjWWQOwZo2o0busBB2766XlWYwCgYI
KoZIzj0EAwMDaAAwZQIwRcp4ZKBsq9XkUuN8wfX+GEbY1N5nmCRc8e80kUkuAefo
uc2j3cICeXo1cOybQ1iWAjEA3Ooawl8eQyR4wrjCofUE8h44p0j7Yl/kBlJZT8+9
vbtH7QiVzeKCOTQPINyRql6P
-----END CERTIFICATE-----"
                        .as_bytes(),
                    )
                    .unwrap(),
                )
        }
        Ok(Self {
            client: builder.build()?,
            is_logged_in,
            api_endpoint,
            options_secret_key: options_secret_key.clone(),
            options_profile: options_profile.clone(),
            options_api_endpoint: options_api_endpoint.clone(),
            secret_key,
            profile,
        })
    }

    pub fn reinit(&mut self) -> anyhow::Result<()> {
        *self = Self::new_inner(
            &self.options_secret_key,
            &self.options_profile,
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
        req: reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let resp = req.send().await.map_err(HttpError)?;
        if !resp.status().is_success() {
            let ErrorResponse { status, error } = resp.json().await.map_err(HttpError)?;
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
        Ok(resp.json().await.map_err(HttpError)?)
    }

    pub async fn get<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
    ) -> anyhow::Result<T> {
        self.request(self.client.get(self.api_endpoint.join(uri.as_ref())?))
            .await
    }

    pub async fn post<T, J>(&self, uri: impl AsRef<str>, body: &J) -> anyhow::Result<T>
        where
            T: serde::de::DeserializeOwned,
            J: serde::Serialize + ?Sized,
    {
        self.request(
            self.client
                .post(self.api_endpoint.join(uri.as_ref())?)
                .json(body),
        )
        .await
    }

    pub async fn put<T, J>(&self, uri: impl AsRef<str>, body: &J) -> anyhow::Result<T>
        where
            T: serde::de::DeserializeOwned,
            J: serde::Serialize + ?Sized,
    {
        self.request(
            self.client
                .put(self.api_endpoint.join(uri.as_ref())?)
                .json(body),
        )
        .await
    }

    pub async fn delete<T: serde::de::DeserializeOwned>(
        &self,
        uri: impl AsRef<str>,
    ) -> anyhow::Result<T> {
        self.request(self.client.delete(self.api_endpoint.join(uri.as_ref())?))
            .await
    }
}

pub fn cloud_config_file(profile: &Option<String>) -> anyhow::Result<PathBuf> {
    Ok(cloud_config_dir()?.join(format!("{}.json", profile.as_deref().unwrap_or("default"))))
}

pub fn cloud_config_dir() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("cloud-credentials"))
}
