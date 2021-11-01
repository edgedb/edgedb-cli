use std::env;
use std::fmt;
use std::iter;
use std::str::FromStr;
use std::time::Duration;

use crate::portable::platform;
use crate::portable::ver;

use anyhow::Context;
use async_std::fs;
use async_std::io::{ReadExt, WriteExt};
use async_std::path::Path;
use async_std::task;
use fn_error_context::context;
use url::Url;
use indicatif::{ProgressBar, ProgressStyle};

const MAX_ATTEMPTS: u32 = 10;
pub const USER_AGENT: &str = "edgedb";

#[derive(Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    // Prerelease,  // TODO(tailhook)
    Nightly,
}

#[derive(Debug, Clone)]
pub enum PackageType {
    TarZst,
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
    pub package_type: PackageType,
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, thiserror::Error)]
#[error("HTTP failure: {} {}",
        self.0.status(), self.0.status().canonical_reason())]
pub struct HttpFailure(surf::Response);


impl FromStr for PackageType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<PackageType> {
        if s.ends_with(".tar.zst") {
            return Ok(PackageType::TarZst);
        }
        anyhow::bail!("Only .tar.zst packages supported");
    }
}

impl PackageType {
    fn as_ext(&self) -> &str {
        match self {
            PackageType::TarZst => ".tar.zst",
        }
    }
}

impl PackageInfo {
    pub fn cache_file_name(&self) -> String {
        // TODO(tailhook) use package hash when that is available
        let hash = blake3::hash(self.package_url.as_str().as_bytes());
        format!("edgedb-server_{}_{:7}{}",
                self.version, hash, self.package_type.as_ext())
    }
}


fn retry_seconds() -> impl Iterator<Item=u64> {
    [5, 15, 30, 60].iter().cloned().chain(iter::repeat(60))
}

pub async fn get_header(original_url: &Url) -> anyhow::Result<surf::Response> {
    use surf::StatusCode::{MovedPermanently, PermanentRedirect};
    use surf::StatusCode::{TooManyRequests};

    let mut url = original_url.clone();
    let mut attempt = 0;
    let mut retry = retry_seconds();

    loop {

        log::info!("Fetching JSON at {}", url);
        match surf::get(&url).header("User-Agent", USER_AGENT).await {
            Ok(res) if res.status().is_success() => {
                break Ok(res);
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
    }
}

#[context("failed to fetch JSON at URL: {}", url)]
pub async fn get_json<T>(url: &Url) -> Result<T, anyhow::Error>
    where T: serde::de::DeserializeOwned,
{
    let body_bytes = get_header(url).await?
        .body_bytes().await.map_err(HttpError)?;

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
                package_type: pkg.installref.parse().ok()?,
            })
        })
        .collect();
    Ok(packages)
}

pub fn get_server_package(channel: Channel, query: &Option<ver::Query>)
    -> anyhow::Result<Option<PackageInfo>>
{
    let query = query.as_ref();
    let pkg = get_server_packages(channel)?.into_iter()
        .filter(|pkg| query.map(|q| q.matches(&pkg.version)).unwrap_or(true))
        .max_by_key(|pkg| pkg.version.specific());
    Ok(pkg)
}

#[context("failed to download file at URL: {}", url)]
pub async fn download(dest: impl AsRef<Path>, url: &Url)
    -> Result<(), anyhow::Error>
{
    let dest = dest.as_ref();
    log::info!("Downloading {} -> {}", url, dest.display());
    let mut body = get_header(url).await?.take_body();
    let mut out = fs::File::create(dest).await
        .with_context(|| format!("writing {:?}", dest.display()))?;

    let bar = if let Some(len) = body.len() {
        ProgressBar::new(len as u64)
    } else {
        ProgressBar::new_spinner()
    };
    bar.set_style(
        ProgressStyle::default_bar()
        .template(
            "{elapsed_precise} [{bar}] \
            {bytes:>7.dim}/{total_bytes:7} \
            {binary_bytes_per_sec:.dim} | ETA: {eta}")
        .progress_chars("=> "));
    let mut buf = [0u8; 16384];
    loop {
        let bytes = body.read(&mut buf).await?;
        if bytes == 0 {
            break;
        }
        out.write_all(&buf[..bytes]).await?;
        bar.inc(bytes as u64);
    }
    bar.finish();

    Ok(())
}

impl fmt::Display for PackageInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "edgdb-server@{}", self.version)
    }
}
