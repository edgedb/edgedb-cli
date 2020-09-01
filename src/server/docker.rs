use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use anyhow::Context;
use async_std::task;
use serde::{Serialize, Deserialize};

use crate::process;
use crate::server::detect::Lazy;
use crate::server::detect::VersionQuery;
use crate::server::distribution::{DistributionRef, Distribution, MajorVersion};
use crate::server::init;
use crate::server::install;
use crate::server::methods::InstallMethod;
use crate::server::os_trait::{CurrentOs, Method};
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

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct Image {
    major_version: MajorVersion,
    version: Version<String>,
    tag: Tag,
}

impl Tag {
    pub fn matches(&self, q: &VersionQuery) -> bool {
        match (self, q) {
            (Tag::Stable(_, _), VersionQuery::Stable(None)) => true,
            (Tag::Stable(t, _), VersionQuery::Stable(Some(q))) => t == q.num(),
            (Tag::Nightly(_), VersionQuery::Nightly) => true,
            _ => false,
        }
    }
    pub fn into_distr(self) -> DistributionRef {
        match &self {
            Tag::Stable(v, rev) => Image {
                major_version: MajorVersion::Stable(Version(v.clone())),
                version: Version(format!("{}-{}", v, rev)),
                tag: self,
            },
            Tag::Nightly(v) => Image {
                major_version: MajorVersion::Nightly,
                version: Version(v.clone()),
                tag: self,
            },
        }.into_ref()
    }
}

impl Distribution for Image {
    fn major_version(&self) -> &MajorVersion {
        &self.major_version
    }
    fn version(&self) -> &Version<String> {
        &self.version
    }
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

impl Tag {
    fn from_pair(name: &str, digest: &str) -> Option<Tag> {
        if name.starts_with("1") {
            // examples: `1-alpha4`, `1.1`
            // digest example: sha256:0d56a04fe70892...
            let rev = digest.split(":").skip(1).next();
            match rev {
                Some(rev) => Some(Tag::Stable(name.into(), rev[..7].into())),
                None => None,
            }
        } else if name.starts_with("202") {
            // example: `20200826052156c04ba5`
            Some(Tag::Nightly(name.into()))
        } else {
            // maybe `latest` or something unrelated
            None
        }
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
                    tags.extend(data.results
                        .into_iter()
                        .flat_map(|t| {
                            t.images.get(0).and_then(|img| {
                                Tag::from_pair(&t.name, &img.digest)
                            })
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
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        let image = settings.distribution.downcast_ref::<Image>()
            .context("invalid distribution for Docker")?;
        process::run(Command::new(&self.cli)
            .arg("image")
            .arg("pull")
            .arg(format!("edgedb/edgedb:{}", match &image.tag {
                Tag::Stable(v, _) => v,
                Tag::Nightly(n) => n,
            })))?;
        Ok(())
    }
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<DistributionRef>>
    {
        use Tag::*;

        if nightly {
            Ok(self.get_tags()?.iter().filter_map(|t| {
                match t {
                    Stable(..) => None,
                    Nightly(v) => {
                        Some(Image {
                            major_version: MajorVersion::Nightly,
                            version: Version(v.clone()),
                            tag: t.clone(),
                        }.into_ref())
                    }
                }
            }).collect())
        } else {
            Ok(self.get_tags()?.iter().filter_map(|t| {
                match t {
                    Stable(v, rev) => Some(Image {
                        major_version: MajorVersion::Stable(Version(v.clone())),
                        version: Version(format!("{}-{}", v, rev)),
                        tag: t.clone(),
                    }.into_ref()),
                    Nightly(..) => None,
                }
            }).collect())
        }
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<DistributionRef>
    {
        let tag = self.get_tags()?
            .iter()
            .filter(|tag| tag.matches(query))
            .max()
            .with_context(|| format!("version {} not found", query))?;
        Ok(tag.clone().into_distr())
    }
    fn installed_versions(&self) -> anyhow::Result<Vec<DistributionRef>> {
        let data = process::get_text(Command::new(&self.cli)
            .arg("image")
            .arg("list")
            .arg("--no-trunc")
            .arg("--format")
            .arg("{{.Repository}} {{.Tag}} {{.Digest}}"))?;
        let mut result = Vec::new();
        for line in data.lines() {
            let mut words = line.split_whitespace();
            if words.next() != Some("edgedb/edgedb") {
                continue;
            }
            match (words.next(), words.next()) {
                (Some(name), Some(digest)) => {
                    if let Some(tag) = Tag::from_pair(name, digest) {
                        result.push(tag.into_distr());
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn is_system_only(&self) -> bool {
        true
    }
    fn get_server_path(&self, _distr: &DistributionRef)
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
