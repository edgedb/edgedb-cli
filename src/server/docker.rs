use std::fmt;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use anyhow::Context;
use async_std::task;
use serde::{Serialize, Deserialize};

use crate::process;
use crate::platform::ProcessGuard;
use crate::platform::home_dir;
use crate::server::detect::Lazy;
use crate::server::detect::VersionQuery;
use crate::server::distribution::{DistributionRef, Distribution, MajorVersion};
use crate::server::init::{self, read_ports, Storage};
use crate::server::init::{bootstrap_script, save_credentials};
use crate::server::install;
use crate::server::options::{StartConf};
use crate::server::methods::InstallMethod;
use crate::server::os_trait::{CurrentOs, Method, Instance, InstanceRef};
use crate::server::remote;
use crate::server::unix;
use crate::server::version::Version;
use crate::server::status::{Service, Status, DataDirectory, BackupStatus};
use crate::server::status::{probe_port};
use crate::server::reset_password::{generate_password};


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

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
pub struct ContainerState {
    Running: bool,
    Pid: u32,
    ExitCode: u16,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
pub struct Container {
    State: ContainerState,
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

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
pub struct DockerVolume {
    Name: String,
    // extra fields are allowed
}

pub struct DockerInstance<'a, O: CurrentOs + ?Sized> {
    method: &'a DockerMethod<'a, O>,
    name: String,
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
    pub fn as_image_name(&self) -> String {
        format!("edgedb/edgedb:{}", match &self {
                Tag::Stable(v, _) => v,
                Tag::Nightly(n) => n,
        })
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
    pub fn inspect_container(&self, name: &str)
        -> anyhow::Result<Option<Container>>
    {
        let mut cmd = Command::new(&self.cli);
        cmd.arg("container");
        cmd.arg("inspect");
        cmd.arg(name);
        match process::get_json_or_failure::<Vec<_>>(&mut cmd)? {
            Ok(containers) => Ok(containers.into_iter().next()),
            Err(e) => {
                if e.contains("No such container") {
                    Ok(None)
                } else {
                    anyhow::bail!("cannot inspect container {}: {}",
                        name, e);
                }
            }
        }
    }
    pub fn inspect_volume(&self, name: &str)
        -> anyhow::Result<Option<DockerVolume>>
    {
        let mut cmd = Command::new(&self.cli);
        cmd.arg("volume");
        cmd.arg("inspect");
        cmd.arg(name);
        match process::get_json_or_failure::<Vec<_>>(&mut cmd)? {
            Ok(imgs) => {
                Ok(imgs.into_iter().next())
            }
            Err(e) => {
                if e.contains("No such volume") {
                    Ok(None)
                } else {
                    anyhow::bail!("cannot lookup docker volume {:?}: {}",
                        name, e);
                }
            }
        }
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
            .arg(image.tag.as_image_name()))?;
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
    fn get_storage(&self, system: bool, name: &str)-> anyhow::Result<Storage> {
        Ok(Storage::DockerVolume(format!("edgedb_{}", name)))
    }
    fn storage_exists(&self, storage: &Storage) -> anyhow::Result<bool> {
        match storage {
            Storage::DockerVolume(name) => {
                match self.inspect_volume(name)? {
                    // TODO(tailhook) check volume's metadata
                    Some(_) => Ok(true),
                    None => Ok(false),
                }
            }
            _ => unix::storage_exists(storage),
        }
    }
    fn clean_storage(&self, storage: &Storage) -> anyhow::Result<()> {
        match storage {
            Storage::DockerVolume(name) => {
                process::run(Command::new(&self.cli)
                    .arg("volume")
                    .arg("rm")
                    .arg(name))?;
                Ok(())
            }
            _ => unix::clean_storage(storage),
        }
    }
    fn bootstrap(&self, settings: &init::Settings) -> anyhow::Result<()> {
        let volume = match &settings.storage {
            Storage::DockerVolume(name) => name,
            other => anyhow::bail!("unsupported storage {:?}", other),
        };
        let image = settings.distribution.downcast_ref::<Image>()
            .context("invalid unix package")?;
        if let Some(upgrade_marker) = &settings.upgrade_marker {
            todo!();
        }
        let md = serde_json::to_string(&settings.metadata())?;
        process::get_text(Command::new(&self.cli)
            .arg("volume")
            .arg("create")
            .arg(volume)
            .arg("--label")
            .arg(format!("com.edgedb.metadata={}", md)))?;

        process::run(Command::new(&self.cli)
            .arg("run")
            .arg("--rm")
            .arg("--mount").arg(format!("source={},target=/mnt", volume))
            .arg(image.tag.as_image_name())
            .arg("chown")
            .arg("999:999")
            .arg("/mnt"))?;

        let password = generate_password();
        let mut cmd = Command::new(&self.cli);
        cmd.arg("run");
        cmd.arg("--rm");
        cmd.arg("--user=999:999");
        cmd.arg("--mount")
           .arg(format!("source={},target=/var/lib/edgedb/data", volume));
        cmd.arg(image.tag.as_image_name());
        cmd.arg("edgedb-server");
        cmd.arg("--bootstrap-only");
        cmd.arg("--bootstrap-commmand")
            .arg(bootstrap_script(settings, &password));
        cmd.arg("--log-level=warn");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", settings.name));
        if settings.inhibit_user_creation {
            cmd.arg("--default-database=edgedb");
            cmd.arg("--default-database-user=edgedb");
        }

        log::debug!("Running bootstrap {:?}", cmd);
        match cmd.status() {
            Ok(s) if s.success() => {}
            Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
            Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
        }

        save_credentials(&settings, &password)?;
        drop(password);

        let mut cmd = Command::new(&self.cli);
        cmd.arg("container");
        match settings.start_conf {
            StartConf::Auto => {
                cmd.arg("run");
                cmd.arg("--detach");
                cmd.arg("--restart=always");
            }
            StartConf::Manual => {
                cmd.arg("create");
            }
        }

        cmd.arg("--user=999:999");
        cmd.arg("--group=999");
        cmd.arg(format!("--publish={0}:{0}", settings.port));
        cmd.arg("--mount")
           .arg(format!("source={},target=/var/lib/edgedb/data", volume));
        cmd.arg(image.tag.as_image_name());
        cmd.arg("edgedb-server");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", settings.name));
        cmd.arg("--port").arg(settings.port.to_string());
        process::run(&mut cmd).with_context(|| {
            match settings.start_conf {
                StartConf::Auto => {
                    format!("error starting server {:?}", cmd)
                }
                StartConf::Manual => {
                    format!("error creating container {:?}", cmd)
                }
            }
        })?;

        Ok(())
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>>
         where Self: 'os
    {
        let output = process::get_text(Command::new(&self.cli)
            .arg("volume")
            .arg("list")
            .arg("--filter").arg("label=com.edgedb.metadata")
            .arg("--format").arg("{{.Name}}"))?;
        let mut result = Vec::new();
        for volume in output.lines() {
            if let Some(name) = volume.strip_prefix("edgedb_") {
                result.push(DockerInstance {
                    name: name.into(),
                    method: self,
                }.into_ref());
            }
        }
        Ok(result)
    }
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>
    {
        unreachable!();
    }
    fn get_instance<'x>(&'x self, name: &str)
        -> anyhow::Result<InstanceRef<'x>>
    {
        let volume = format!("edgedb_{}", name);
        match self.inspect_volume(&volume)? {
            Some(_) => {
                Ok(DockerInstance {
                    name: name.into(),
                    method: self,
                }.into_ref())
            }
            None => anyhow::bail!("No docker volume {:?} found", volume),
        }
    }
}


impl<O: CurrentOs + ?Sized> Instance for DockerInstance<'_, O> {
    fn name(&self) -> &str {
        &self.name
    }
    fn get_status(&self) -> Status {
        use DataDirectory::*;
        use Service::*;

        let container_name = format!("edgedb_{}", self.name);
        let (service, service_exists) =
            match self.method.inspect_container(&container_name) {
                Ok(Some(info)) => {
                    if info.State.Running {
                        (Running { pid: info.State.Pid }, true)
                    } else {
                        (Failed { exit_code: Some(info.State.ExitCode) }, true)
                    }
                }
                Ok(None) => (Inactive { error: "Not found".into() }, false),
                Err(e) => (Inactive { error: e.to_string() }, false),
            };

        /*
        let (data_status, metadata) = if data_dir.exists() {
            let metadata = read_metadata(&data_dir);
            if metadata.is_ok() {
                let upgrade_file = data_dir.join("UPGRADE_IN_PROGRESS");
                if upgrade_file.exists() {
                    (Upgrading(read_upgrade(&upgrade_file)), metadata)
                } else {
                    (Normal, metadata)
                }
            } else {
                (NoMetadata, metadata)
            }
        } else {
            (Absent, Err(anyhow::anyhow!("No data directory")))
        };
        */
        let data_status = NoMetadata; // TODO
        let metadata = Err(anyhow::anyhow!("unimplemented")); // TODO
        let reserved_port =
            // TODO(tailhook) cache ports
            read_ports()
            .map_err(|e| log::warn!("{:#}", e))
            .ok()
            .and_then(|ports| ports.get(&self.name).cloned());
        let port_status = probe_port(&metadata, &reserved_port);
        //let backup = backup_status(&base.join(format!("{}.backup", name)));
        let backup = BackupStatus::Absent;
        let credentials_file_exists = home_dir().map(|home| {
            home.join(".edgedb")
                .join("credentials")
                .join(format!("{}.json", self.name))
                .exists()
        }).unwrap_or(false);

        Status {
            name: self.name.clone(),
            service,
            metadata,
            reserved_port,
            port_status,
            data_dir: "todo".into(), // TODO
            data_status,
            backup,
            service_exists,
            credentials_file_exists,
        }
    }
}

impl<O: CurrentOs + ?Sized> fmt::Debug for DockerInstance<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let DockerInstance {
            name,
            method: _method,
        } = self;
        f.debug_struct("DockerInstance")
            .field("name", name)
            .finish()
    }
}

