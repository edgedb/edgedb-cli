use std::iter;
use std::env;
use std::time::Duration;

use crate::portable::platform;
use crate::portable::ver;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;
use url::Url;

const MAX_ATTEMPTS: u32 = 10;
pub const USER_AGENT: &str = "edgedb";

#[derive(Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    // Prerelease,  // TODO(tailhook)
    Nightly,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct RepositoryData {
    pub packages: Vec<PackageData>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct PackageData {
    pub basename: String,
    pub version: String,
    pub installref: String,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub version: ver::Build,
    pub package_url: Url,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, thiserror::Error)]
#[error("HTTP failure: {} {}",
        self.0.status(), self.0.status().canonical_reason())]
pub struct HttpFailure(surf::Response);


fn retry_seconds() -> impl Iterator<Item=u64> {
    [5, 15, 30, 60].iter().cloned().chain(iter::repeat(60))
}

#[context("failed to fetch JSON at URL: {}", original_url)]
pub async fn get_json<T>(original_url: &Url) -> Result<T, anyhow::Error>
    where T: serde::de::DeserializeOwned,
{
    use surf::StatusCode::{MovedPermanently, PermanentRedirect};
    use surf::StatusCode::{TooManyRequests};

    let mut url = original_url.clone();
    let mut attempt = 0;
    let mut retry = retry_seconds();

    let body_bytes = loop {

        log::info!("Fetching JSON at {}", url);
        match surf::get(&url).header("User-Agent", USER_AGENT).await {
            Ok(mut res) if res.status().is_success() => {
                break res.body_bytes().await.map_err(HttpError)?;
            }
            Ok(res) if res.status().is_redirection() => {
                let location = match res.header("Location") {
                    Some(val) => val.last().as_str(),
                    None => anyhow::bail!("unexpected redirect kind {}",
                                          res.status()),
                };
                log::debug!("Redirecting on {} to {}", res.status(), location);
                let new_url = match Url::parse(location) {
                    Ok(url) => url,
                    Err(url::ParseError::RelativeUrlWithoutBase) => {
                        url.join(location)?
                    }
                    Err(e) => return Err(e)?,
                };
                if matches!(res.status(), MovedPermanently | PermanentRedirect)
                {
                    log::warn!("Location {} permanently moved to {}.",
                               url, new_url);
                }
                url = new_url;
            }
            Ok(res) if res.status().is_server_error() ||
                       res.status() == TooManyRequests
            => {
                let secs = retry.next().unwrap();
                log::warn!("Error fetching {}: {}. Will retry in {} seconds.",
                           url, res.status(), secs);
                task::sleep(Duration::from_secs(secs)).await;
            }
            Ok(res) => return Err(HttpFailure(res))?,
            Err(e) => return Err(HttpError(e))?,
        }
        attempt += 1;
        if attempt > MAX_ATTEMPTS {
            anyhow::bail!("too many attempts");
        }
    };

    let jd = &mut serde_json::Deserializer::from_slice(&body_bytes);
    Ok(serde_path_to_error::deserialize(jd)?)
}

pub fn get_server_packages(channel: Channel)
    -> anyhow::Result<Vec<PackageInfo>>
{
    use Channel::*;

    let pkg_root = env::var("EDGEDB_PKG_ROOT")
        .unwrap_or_else(|_| String::from("https://packages.edgedb.com"));
    let pkg_root = Url::parse(&pkg_root)
        .context("Package root is a valid URL")?;

    let plat = platform::get_name()?;
    let url = pkg_root.join(&match channel {
        Stable => format!("/archive/.jsonindexes/{}.json", plat),
        Nightly => format!("/archive/.jsonindexes/{}.nightly.json", plat),
    })?;
    let data: RepositoryData = task::block_on(get_json(&url))?;
    let packages = data.packages.into_iter()
        .filter(|pkg| pkg.basename == "edgedb-server")
        .filter_map(|pkg| {
            Some(PackageInfo {
                // TODO(tailhook) probably warning on invalid ver/ref
                version: pkg.version.parse().ok()?,
                package_url: pkg_root.join(&pkg.installref).ok()?,
            })
        })
        .collect();
    Ok(packages)
}
