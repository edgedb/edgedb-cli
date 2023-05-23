use std::cmp::min;
use std::env;
use std::fmt;
use std::time::Duration;
use std::future;
use std::path::Path;

use anyhow::Context;
use fn_error_context::context;
use indicatif::{ProgressBar, ProgressStyle};
use is_terminal::IsTerminal;
use once_cell::sync::OnceCell;
use serde::{ser, de, Serialize, Deserialize};
use tokio::fs;
use tokio::io::{AsyncWriteExt};
use url::Url;

use crate::async_util::timeout;
use crate::portable::platform;
use crate::portable::ver;
use crate::portable::windows;


pub const USER_AGENT: &str = "edgedb";
pub const DEFAULT_TIMEOUT: Duration = Duration::new(60, 0);
static PKG_ROOT: OnceCell<Url> = OnceCell::new();

#[derive(thiserror::Error, Debug)]
#[error("page not found")]
pub struct NotFound;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[derive(clap::ValueEnum)]
#[clap(value(rename_all="kebab-case"))]
pub enum Channel {
    Stable,
    Testing,
    Nightly,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum PackageType {
    TarZst,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum Compression {
    Zstd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub channel: Channel,
    pub version: Option<ver::Filter>,
}

pub struct QueryOptions<'a> {
    pub stable: bool,
    pub nightly: bool,
    pub testing: bool,
    pub channel: Option<Channel>,
    pub version: Option<&'a ver::Filter>,
}

#[derive(Debug, Clone)]
pub struct QueryDisplay<'a>(&'a Query);

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct PackageInfo {
    pub version: ver::Build,
    pub url: Url,
    pub size: u64,
    pub hash: PackageHash,
    pub kind: PackageType,
}

#[derive(Debug, Clone)]
pub struct CliPackageInfo {
    pub version: ver::Semver,
    pub url: Url,
    pub size: u64,
    pub hash: PackageHash,
    pub compression: Option<Compression>,
}

#[derive(Debug, Clone)]
pub enum PackageHash {
    Blake2b(Box<str>),
    Unknown(Box<str>),
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
        let hash = self.hash.short();
        format!("edgedb-server_{}_{:7}{}",
                self.version, hash, self.kind.as_ext())
    }
}

fn pkg_root() -> anyhow::Result<&'static Url> {
    PKG_ROOT.get_or_try_init(|| {
        let pkg_root = env::var("EDGEDB_PKG_ROOT")
            .unwrap_or_else(|_| String::from("https://packages.edgedb.com"));
        let pkg_root = Url::parse(&pkg_root)
            .context("Package root is a valid URL")?;
        Ok(pkg_root)
    })
}

fn channel_suffix(channel: Channel) -> &'static str {
    use Channel::*;

    match channel {
        Stable => "",
        Nightly => ".nightly",
        Testing => ".testing",
    }
}

fn json_url(platform: &str, channel: Channel) -> anyhow::Result<Url> {
    Ok(pkg_root()?.join(
        &format!("/archive/.jsonindexes/{}{}.json",
                 platform,
                 channel_suffix(channel))
    )?)
}

async fn _get_json<T>(url: &Url) -> Result<T, anyhow::Error>
    where T: serde::de::DeserializeOwned,
{
    log::info!("Fetching JSON at {}", url);
    let body_bytes = reqwest::Client::new().get(url.clone())
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send().await?
        .error_for_status()?
        .bytes().await?;

    let jd = &mut serde_json::Deserializer::from_slice(&body_bytes);
    Ok(serde_path_to_error::deserialize(jd)?)
}

#[context("failed to fetch JSON at URL: {}", url)]
#[tokio::main]
async fn get_json<T>(url: &Url, timeo: Duration) -> Result<T, anyhow::Error>
    where T: serde::de::DeserializeOwned,
{
    tokio::select! {
        res = timeout(timeo, _get_json(url)) => res,
        _ = async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if std::io::stderr().is_terminal() {
                eprintln!("Fetching {} takes too long. Common reasons are:",
                          url);
                eprintln!("  1. Your internet connectivity is slow");
                eprintln!("  2. Firewall blocks internet access to this resource");
                if windows::is_in_wsl() {
                    eprintln!("Note: EdgeDB CLI tool is running in \
                               Windows Subsystem for Linux (WSL).");
                    eprintln!("  Consider adding Windows Defender Firewall \
                              rule for WSL.");
                }
            }
            future::pending().await
        } => unreachable!(),
    }
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

fn filter_cli_package(pkg_root: &Url, pkg: &PackageData)
    -> Option<CliPackageInfo>
{
    let result = _filter_cli_package(pkg_root, pkg);
    if result.is_none() {
        log::info!("Skipping package {:?}", pkg);
    }
    return result;
}

fn _filter_cli_package(pkg_root: &Url, pkg: &PackageData)
    -> Option<CliPackageInfo>
{
    let iref = pkg.installrefs.iter()
        .filter(|r| (
                r.encoding.as_ref().map(|x| &x[..]) == Some("zstd") &&
                r.verification.blake2b.as_ref()
                    .map(valid_hash).unwrap_or(false)
        ))
        .next()
        .or_else(|| {
            pkg.installrefs.iter()
            .filter(|r| (
                    r.encoding.as_ref().map(|x| &x[..]) == Some("identity") &&
                    r.verification.blake2b.as_ref()
                        .map(valid_hash).unwrap_or(false)
            ))
            .next()
        })?;
    let cmpr = if iref.encoding.as_ref().map(|x| &x[..]) == Some("zstd") {
        Some(Compression::Zstd)
    } else {
        None
    };
    Some(CliPackageInfo {
        version: pkg.version.parse().ok()?,
        url: pkg_root.join(&iref.path).ok()?,
        hash: PackageHash::Blake2b(
            iref.verification.blake2b.as_ref()?[..].into()),
        compression: cmpr,
        size: iref.verification.size,
    })
}

fn valid_hash(val: &String) -> bool {
    val.len() == 128 &&
        hex::decode(val).map(|x| x.len() == 64).unwrap_or(false)
}

pub fn get_cli_packages(channel: Channel, timeout: Duration)
    -> anyhow::Result<Vec<CliPackageInfo>>
{
    get_platform_cli_packages(channel, platform::get_cli()?, timeout)
}

pub fn get_platform_cli_packages(channel: Channel, platform: &str,
                                 timeo: Duration)
    -> anyhow::Result<Vec<CliPackageInfo>>
{
    let pkg_root = pkg_root()?;
    let data: RepositoryData;
    data = match get_json(&json_url(platform, channel)?, timeo) {
        Ok(data) => data,
        Err(e) if e.is::<NotFound>() => RepositoryData { packages: vec![] },
        Err(e) => return Err(e),
    };
    let packages = data.packages.iter()
        .filter(|pkg| pkg.basename == "edgedb-cli")
        .filter_map(|p| filter_cli_package(&pkg_root, p))
        .collect();
    Ok(packages)
}

pub fn get_server_packages(channel: Channel)
    -> anyhow::Result<Vec<PackageInfo>>
{
    let plat = platform::get_server()?;
    get_platform_server_packages(channel, plat)
}

fn get_platform_server_packages(channel: Channel, platform: &str)
    -> anyhow::Result<Vec<PackageInfo>>
{
    let pkg_root = pkg_root()?;
    let data: RepositoryData;
    data = match get_json(&json_url(platform, channel)?, DEFAULT_TIMEOUT) {
        Ok(data) => data,
        Err(e) if e.is::<NotFound>() => RepositoryData { packages: vec![] },
        Err(e) => return Err(e),
    };
    let packages = data.packages.iter()
        .filter(|pkg| pkg.basename == "edgedb-server")
        .filter_map(|p| filter_package(&pkg_root, p))
        .collect();
    Ok(packages)
}

pub fn get_server_package(query: &Query)
    -> anyhow::Result<Option<PackageInfo>>
{
    let plat = platform::get_server()?;
    if cfg!(all(target_arch="aarch64", target_os="macos")) &&
        query.version.as_ref().map(|v| v.major == 1).unwrap_or(false)
    {
        return get_platform_server_package(query, "x86_64-apple-darwin");
    }
    get_platform_server_package(query, plat)
}

fn get_platform_server_package(query: &Query, platform: &str)
    -> anyhow::Result<Option<PackageInfo>>
{
    let filter = query.version.as_ref();
    let pkg = get_platform_server_packages(query.channel, platform)?
        .into_iter()
        .filter(|pkg| filter.map(|q| q.matches(&pkg.version)).unwrap_or(true))
        .max_by_key(|pkg| pkg.version.specific());
    Ok(pkg)
}

pub fn get_specific_package(version: &ver::Specific)
    -> anyhow::Result<Option<PackageInfo>>
{
    let channel = Channel::from_version(version)?;
    let all = if
        cfg!(all(target_arch="aarch64", target_os="macos")) &&
        version.major == 1
    {
        get_platform_server_packages(channel, "x86_64-apple-darwin")?
    } else {
        get_server_packages(channel)?
    };
    let pkg = all.into_iter()
        .filter(|pkg| &pkg.version.specific() == version)
        .next();
    Ok(pkg)
}

#[context("failed to download file at URL: {}", url)]
#[tokio::main]
pub async fn download(dest: impl AsRef<Path>, url: &Url, quiet: bool)
    -> Result<blake2b_simd::Hash, anyhow::Error>
{
    let dest = dest.as_ref();
    log::info!("Downloading {} -> {}", url, dest.display());
    let mut req = reqwest::Client::new().get(url.clone())
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .send().await?
        .error_for_status()?;
    let mut out = fs::File::create(dest).await
        .with_context(|| format!("writing {:?}", dest.display()))?;

    let bar = if quiet {
        ProgressBar::hidden()
    } else if let Some(len) = req.content_length() {
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
        .expect("template is ok")
        .progress_chars("=> "));
    let mut hasher = blake2b_simd::State::new();
    while let Some(chunk) = req.chunk().await? {
        out.write_all(&chunk[..]).await?;
        hasher.update(&chunk[..]);
        bar.inc(chunk.len() as u64);
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
    pub fn nightly() -> Query {
        Query { channel: Channel::Nightly, version: None }
    }
    pub fn stable() -> Query {
        Query { channel: Channel::Stable, version: None }
    }
    pub fn testing() -> Query {
        Query { channel: Channel::Testing, version: None }
    }
    pub fn display(&self) -> QueryDisplay {
        QueryDisplay(self)
    }
    pub fn from_options(opt: QueryOptions,
                        default: impl FnOnce() -> anyhow::Result<Query>)
        -> anyhow::Result<(Query, bool)>
    {
        let QueryOptions { stable, nightly, testing, channel, version } = opt;
        if let Some(ver) = version {
            Ok((Query::from_filter(ver)?, true))
        } else if let Some(channel) = channel {
            Ok((Query { channel, version: None }, true))
        } else if nightly {
            Ok((Query::nightly(), true))
        } else if testing {
            Ok((Query::testing(), true))
        } else if stable {
            Ok((Query::stable(), true))
        } else {
            Ok((default()?, false))
        }
    }
    pub fn from_filter(ver: &ver::Filter) -> anyhow::Result<Query> {
        use crate::portable::repository::ver::FilterMinor;
        match ver.minor {
            None => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver.clone()),
            }),
            Some(FilterMinor::Alpha(_)) |
            Some(FilterMinor::Beta(_)) |
            Some(FilterMinor::Rc(_))
            if ver.major == 1 || ver.major == 2 => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver.clone()),
            }),
            Some(FilterMinor::Alpha(_)) |
            Some(FilterMinor::Beta(_)) |
            Some(FilterMinor::Rc(_)) => Ok(Query {
                channel: Channel::Testing,
                version: Some(ver.clone()),
            }),
            Some(FilterMinor::Minor(_)) => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver.clone()),
            }),
        }
    }
    pub fn from_version(ver: &ver::Specific) -> anyhow::Result<Query> {
        use crate::portable::repository::ver::{MinorVersion, FilterMinor};
        match ver.minor {
            MinorVersion::Dev(_) => Ok(Query::nightly()),
            MinorVersion::Alpha(v) if ver.major == 1 => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Alpha(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Beta(v) if ver.major == 1 => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Beta(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Rc(v) if ver.major == 1 || ver.major == 2 => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Rc(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Minor(v) => Ok(Query {
                channel: Channel::Stable,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Minor(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Alpha(v) =>  Ok(Query {
                channel: Channel::Testing,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Alpha(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Beta(v) =>  Ok(Query {
                channel: Channel::Testing,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Beta(v)),
                    exact: false,
                }),
            }),
            MinorVersion::Rc(v) =>  Ok(Query {
                channel: Channel::Testing,
                version: Some(ver::Filter {
                    major: ver.major,
                    minor: Some(FilterMinor::Rc(v)),
                    exact: false,
                }),
            }),
        }
    }
    pub fn matches(&self, ver: &ver::Build) -> bool {
        match &self.version {
            Some(query_ver) => query_ver.matches(ver),
            None => {
                Channel::from_version(&ver.specific())
                    .map(|channel| self.channel == channel)
                    .unwrap_or(false)
            }
        }
    }
    pub fn as_config_value(&self) -> String {
        if self.channel == Channel::Nightly {
            "nightly".into()
        } else if let Some(ver) = &self.version {
            ver.to_string()
        } else {
            "*".into()
        }
    }
    pub fn is_nightly(&self) -> bool {
        matches!(self.channel, Channel::Nightly)
    }
    pub fn is_nonrecursive_access_policies_needed(&self) -> bool {
        self.version.as_ref().map(|f| match (f.major, f.minor) {
            (1, _) => false,
            (2, Some(v)) if v < ver::FilterMinor::Minor(6) => false,
            (2, _) => true,
            _ => false,
        }).unwrap_or(true)
    }
    pub fn cli_channel(&self) -> Option<Channel> {
        // Only one argument in CLI is allowed
        // So we skip channel if version is set, since version unambiguously
        // determines the channel. But if there is no version we must provide
        // channel.
        if self.version.is_some() {
            None
        } else {
            Some(self.channel)
        }
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

impl Serialize for Channel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PackageHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        if let Some(hash) = s.strip_prefix("blake2b:") {
            if hash.len() != 128 {
                return Err(de::Error::custom("invalid blake2b hash length"));
            }
            return Ok(PackageHash::Blake2b(hash.into()));
        }
        return Ok(PackageHash::Unknown(s.into()));
    }
}

impl std::str::FromStr for Query {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Query> {
        if s == "*" {
            return Ok(Query {
                channel: Channel::Stable,
                version: None,
            });
        } else if s == "nightly" {
            return Ok(Query {
                channel: Channel::Nightly,
                version: None,
            });
        } else {
            let ver: ver::Filter = s.parse()?;
            return Ok(Query {
                channel: Channel::from_filter(&ver)?,
                version: Some(ver),
            });
        }
    }
}

impl From<Channel> for Query {
    fn from(channel: Channel) -> Query {
        Query {
            channel,
            version:None,
        }
    }
}

impl<'de> Deserialize<'de> for Query {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Channel {
    pub fn from_version(ver: &ver::Specific) -> anyhow::Result<Channel> {
        use ver::MinorVersion::*;
        match ver.minor {
            Dev(_) => Ok(Channel::Nightly),
            Minor(_) => Ok(Channel::Stable),
            Alpha(_) | Beta(_) | Rc(_) if ver.major == 1 || ver.major == 2 => {
                // before 2.0 all prereleases go into a stable channel
                Ok(Channel::Stable)
            }
            Alpha(_) => Ok(Channel::Testing),
            Beta(_) => Ok(Channel::Testing),
            Rc(_) => Ok(Channel::Testing),
        }
    }
    pub fn from_filter(ver: &ver::Filter) -> anyhow::Result<Channel> {
        use ver::FilterMinor::*;
        match ver.minor {
            None => Ok(Channel::Stable),
            Some(Minor(_)) => Ok(Channel::Stable),
            Some(Alpha(_) | Beta(_) | Rc(_))
                if ver.major == 1 || ver.major == 2
            => {
                // before 1.0 all prereleases go into a stable channel
                Ok(Channel::Stable)
            }
            Some(Alpha(_) | Beta(_) | Rc(_)) => Ok(Channel::Testing),
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Channel::Nightly => "nightly",
            Channel::Stable => "stable",
            Channel::Testing => "testing",
        }
    }
}

impl fmt::Display for QueryDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ver::FilterMinor::*;

        match &self.0.version {
            None => self.0.channel.as_str().fmt(f),
            Some(ver) => {
                ver.major.fmt(f)?;
                f.write_str(".")?;
                match ver.minor {
                    None => "0".fmt(f),
                    Some(Minor(m)) => m.fmt(f),
                    Some(Alpha(v)) => write!(f, "0-alpha.{}", v),
                    Some(Beta(v)) => write!(f, "0-beta.{}", v),
                    Some(Rc(v)) => write!(f, "0-rc.{}", v),
                }
            }
        }
    }
}
