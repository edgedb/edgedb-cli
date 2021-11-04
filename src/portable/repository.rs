use std::cmp::min;
use std::env;
use std::fmt;
use std::iter;
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
use serde::{ser, de, Serialize, Deserialize};

const MAX_ATTEMPTS: u32 = 10;
pub const USER_AGENT: &str = "edgedb";


#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Channel {
    Stable,
    // Prerelease,  // TODO(tailhook)
    Nightly,
}

#[derive(Debug, Clone)]
pub enum PackageType {
    TarZst,
}

pub struct Query {
    channel: Channel,
    version: Option<ver::Filter>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RepositoryData {
    pub packages: Vec<PackageData>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct InstallRef {
    #[serde(rename="ref")]
    path: String,
    #[serde(rename="type")]
    kind: String,
    encoding: Option<String>,
    verification: Verification,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageData {
    pub basename: String,
    pub version: String,
    pub installrefs: Vec<InstallRef>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Verification {
    size: u64,
    blake2b: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub version: ver::Build,
    pub url: Url,
    pub size: u64,
    pub hash: PackageHash,
    pub kind: PackageType,
}

#[derive(Debug, Clone)]
pub enum PackageHash {
    Blake2b(Box<str>),
    Unknown(Box<str>),
}

#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, thiserror::Error)]
#[error("HTTP failure: {} {}",
        self.0.status(), self.0.status().canonical_reason())]
pub struct HttpFailure(surf::Response);


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
        let hash = self.hash.short();
        format!("edgedb-server_{}_{:7}{}",
                self.version, hash, self.kind.as_ext())
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

fn filter_package(pkg_root: &Url, pkg: &PackageData) -> Option<PackageInfo> {
    let result = _filter_package(pkg_root, pkg);
    if result.is_none() {
        log::info!("Skipping package {:?}", pkg);
    }
    return result;
}
fn _filter_package(pkg_root: &Url, pkg: &PackageData) -> Option<PackageInfo> {
    let iref = pkg.installrefs.iter()
        .filter(|r| (
                r.kind == "application/x-tar" &&
                r.encoding.as_ref().map(|x| &x[..]) == Some("zstd") &&
                r.verification.blake2b.as_ref()
                    .map(valid_hash).unwrap_or(false)
        ))
        .next()?;
    Some(PackageInfo {
        version: pkg.version.parse().ok()?,
        url: pkg_root.join(&iref.path).ok()?,
        hash: PackageHash::Blake2b(
            iref.verification.blake2b.as_ref()?[..].into()),
        kind: PackageType::TarZst,
        size: iref.verification.size,
    })
}

fn valid_hash(val: &String) -> bool {
    val.len() == 128 &&
        hex::decode(val).map(|x| x.len() == 64).unwrap_or(false)
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
    let packages = data.packages.iter()
        .filter(|pkg| pkg.basename == "edgedb-server")
        .filter_map(|p| filter_package(&pkg_root, p))
        .collect();
    Ok(packages)
}

pub fn get_server_package(query: &Query)
    -> anyhow::Result<Option<PackageInfo>>
{
    let filter = query.version.as_ref();
    let pkg = get_server_packages(query.channel)?.into_iter()
        .filter(|pkg| filter.map(|q| q.matches(&pkg.version)).unwrap_or(true))
        .max_by_key(|pkg| pkg.version.specific());
    Ok(pkg)
}

#[context("failed to download file at URL: {}", url)]
pub async fn download(dest: impl AsRef<Path>, url: &Url)
    -> Result<blake2b_simd::Hash, anyhow::Error>
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
    let mut hasher = blake2b_simd::State::new();
    let mut buf = [0u8; 16384];
    loop {
        let bytes = body.read(&mut buf).await?;
        if bytes == 0 {
            break;
        }
        out.write_all(&buf[..bytes]).await?;
        hasher.update(&buf[..bytes]);
        bar.inc(bytes as u64);
    }
    bar.finish();

    Ok(hasher.finalize())
}

impl fmt::Display for PackageInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "edgdb-server@{}", self.version)
    }
}

impl PackageHash {
    fn short(&self) -> &str {
        match self {
            PackageHash::Blake2b(val) => &val[..7],
            PackageHash::Unknown(val) => {
                let start = val.find(":")
                    .unwrap_or(val.len().saturating_sub(7));
                &val[start..min(7, val.len() - start)]
            }
        }
    }
}

impl fmt::Display for PackageHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PackageHash::Blake2b(val) => write!(f, "blake2b:{}", val),
            PackageHash::Unknown(val) => write!(f, "{}", val),
        }
    }
}

impl Query {
    pub fn from_options(nightly: bool,
        version: &Option<crate::server::version::Version<String>>)
        -> anyhow::Result<Query>
    {
        let channel = if nightly {
            Channel::Nightly
        } else {
            Channel::Stable
        };
        let version = version.as_ref().map(|x| x.num().parse())
            .transpose().context("Unexpected --version")?;

        Ok(Query { channel, version })
    }
}

impl Serialize for PackageHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PackageHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        if let Some(hash) = s.strip_prefix("blake2b:") {
            if hash.len() != 64 {
                return Err(de::Error::custom("invalid blake2b hash length"));
            }
            return Ok(PackageHash::Blake2b(hash.into()));
        }
        return Ok(PackageHash::Unknown(s.into()));
    }
}
