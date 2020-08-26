use std::path::PathBuf;
use std::time::SystemTime;

use async_std::task;
use serde::{Serialize, Deserialize};

use crate::server::detect::Lazy;
use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::init;
use crate::server::install;
use crate::server::methods::InstallMethod;
use crate::server::os_trait::{CurrentOs, Method, PreciseVersion};
use crate::server::remote;
use crate::server::version::Version;


#[derive(Debug, Serialize)]
pub struct DockerCandidate {
    pub supported: bool,
    pub platform_supported: bool,
    cli: Option<PathBuf>,
    socket: Option<PathBuf>,
    socket_permissions_ok: bool,
}

#[derive(Debug)]
pub enum Tag {
    Stable(String, String),
    Nightly(String),
}

#[derive(Debug, Deserialize)]
pub struct TagList {
    count: u64,
    next: String,
    previous: Option<String>,
    results: Vec<TagInfo>,
}

#[derive(Debug, Deserialize)]
pub struct ImageInfo {
    architecture: String,
    features: String,
    digest: String,
    os: String,
    os_features: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
pub struct TagInfo {
    creator: u64,
    id: u64,
    #[serde(with="humantime_serde")]
    last_updated: SystemTime,
    last_updater: u64,
    last_updater_username: String,
    name: String,
    repository: u64,
    full_size: u64,
    v2: bool,
    images: Vec<ImageInfo>,
}

#[derive(Debug, Serialize)]
pub struct DockerMethod<'os, O: CurrentOs + ?Sized> {
    #[serde(skip)]
    os: &'os O,
    cli: PathBuf,
    #[serde(skip)]
    tags: Lazy<Vec<Tag>>,
}

impl DockerCandidate {
    pub fn detect() -> anyhow::Result<DockerCandidate> {
        let cli = which::which("docker").ok();
        let supported = cli.is_some();  // TODO(tailhook) check socket
        Ok(DockerCandidate {
            supported,
            platform_supported: cfg!(unix) || cfg!(windows),
            cli,
            socket: None,  // TODO(tailhook)
            socket_permissions_ok: true,
        })
    }
    pub fn format_option(&self, buf: &mut String, recommended: bool) {
        buf.push_str(
            " * --method=docker -- to install into a Docker container");
        if recommended {
            buf.push_str(" (recommended)");
        }
        buf.push('\n');
    }

    pub fn format_error(&self, buf: &mut String) {
        use std::fmt::Write;
        if self.platform_supported {
            write!(buf,
                " * Note: Error initializing Docker method. \
                Command-line tool: {cli}, docker socket: {sock}",
                cli=if self.cli.is_some() { "found" } else { "not found" },
                sock=if self.socket.is_some() {
                    if self.socket_permissions_ok {
                        "found"
                    } else {
                        "access forbidden"
                    }
                } else {
                    "not found"
                },
            ).unwrap();
        } else {
            buf.push_str(" * Note: Docker is not supported for this platform");
        }
        buf.push('\n');
    }
    pub fn make_method<'os, O>(&self, os: &'os O)
        -> anyhow::Result<DockerMethod<'os, O>>
        where O: CurrentOs + ?Sized,
    {
        if !self.supported {
            anyhow::bail!("Method `docker` is not supported");
        }
        Ok(DockerMethod {
            os,
            cli: self.cli.as_ref().unwrap().clone(),
            tags: Lazy::lazy(),
        })
    }
}

impl<'os, O: CurrentOs + ?Sized> DockerMethod<'os, O> {
    fn get_tags(&self) -> anyhow::Result<&[Tag]> {
        self.tags.get_or_try_init(|| {
            task::block_on(async {
                let mut url = "https://registry.hub.docker.com/\
                        v2/repositories/edgedb/edgedb/tags\
                        ?page_size=1000".to_string();
                let mut tags = Vec::new();
                loop {
                    let data: TagList = remote::get_json(&url,
                        "failed to fetch tag list from the Docker registry"
                    ).await?;
                    let last_page = data.results.len() < 1000;
                    tags.extend(data.results.into_iter().flat_map(|t| {
                        if t.name.starts_with("1") {
                            // examples: `1-alpha4`, `1.1`
                            let rev = t.images.get(0)
                                .and_then(|img| {
                                    img.digest.split(":").skip(1).next()
                                });
                            match rev {
                                Some(rev) => {
                                    Some(Tag::Stable(t.name, rev[..7].into()))
                                }
                                None => None,
                            }
                        } else if t.name.starts_with("202") {
                            // example: `20200826052156c04ba5`
                            Some(Tag::Nightly(t.name))
                        } else {
                            // maybe `latest` or something unrelated
                            None
                        }
                    }));
                    if last_page {
                        break;
                    }
                    url = data.next;
                }
                Ok(tags)
            })
        }).map(|v| &v[..])
    }
}

impl<'os, O: CurrentOs + ?Sized> Method for DockerMethod<'os, O> {
    fn name(&self) -> InstallMethod {
        InstallMethod::Docker
    }
    fn install(&self, _settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        anyhow::bail!("Docker support is not implemented yet"); // TODO
    }
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<PreciseVersion>>
    {
        use Tag::*;

        if nightly {
            Ok(self.get_tags()?.iter().filter_map(|t| {
                match t {
                    Stable(..) => None,
                    Nightly(v) => Some(PreciseVersion::nightly(v)),
                }
            }).collect())
        } else {
            Ok(self.get_tags()?.iter().filter_map(|t| {
                match t {
                    Stable(v, rev) => Some(PreciseVersion::from_pair(v, rev)),
                    Nightly(..) => None,
                }
            }).collect())
        }
    }
    fn get_version(&self, _query: &VersionQuery)
        -> anyhow::Result<VersionResult>
    {
        anyhow::bail!("Docker support is not implemented yet"); // TODO
    }
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]> {
        Ok(&[])
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn is_system_only(&self) -> bool {
        true
    }
    fn get_server_path(&self, _major_version: &Version<String>)
        -> anyhow::Result<PathBuf>
    {
        anyhow::bail!("Cannot directly run dockerized server");
    }
    fn create_user_service(&self, _settings: &init::Settings)
        -> anyhow::Result<()>
    {
        anyhow::bail!("Docker support is not implemented yet"); // TODO
    }
}
