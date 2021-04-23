use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Child, Stdio};
use std::time::{SystemTime, Duration, UNIX_EPOCH};

use anyhow::Context;
use async_std::task;
use edgedb_client as client;
use edgeql_parser::helpers::quote_string;
use fn_error_context::context;
use linked_hash_map::LinkedHashMap;
use serde::{Serialize, Deserialize};

use crate::credentials::{self, get_connector};
use crate::process;
use crate::platform::home_dir;

use crate::commands::ExitCode;
use crate::server::detect::Lazy;
use crate::server::detect::VersionQuery;
use crate::server::distribution::{DistributionRef, Distribution, MajorVersion};
use crate::server::errors::InstanceNotFound;
use crate::server::init::{self, read_ports, Storage};
use crate::server::init::{bootstrap_script, save_credentials};
use crate::server::install;
use crate::server::options::{Start, Stop, Restart, Upgrade, Destroy, Logs};
use crate::server::options::{StartConf};
use crate::server::metadata::Metadata;
use crate::server::methods::InstallMethod;
use crate::server::os_trait::{CurrentOs, Method, Instance, InstanceRef};
use crate::server::remote;
use crate::server::unix;
use crate::server::upgrade;
use crate::server::version::Version;
use crate::server::status::{Service, Status, DataDirectory, BackupStatus};
use crate::server::status::{probe_port};
use crate::server::reset_password::{generate_password};


#[derive(Debug, Serialize)]
pub struct DockerCandidate {
    pub supported: bool,
    pub platform_supported: bool,
    cli: Option<PathBuf>,
    docker_info_worked: bool,
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
    features: Option<String>,
    digest: String,
    os: String,
    os_features: Option<String>,
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
pub struct ContainerLabels {
    #[serde(rename="com.edgedb.upgrade-in-progress")]
    upgrading: Option<String>,
    #[serde(rename="com.edgedb.metadata.user")]
    user: Option<String>,
    #[serde(rename="com.edgedb.metadata.port")]
    port: Option<String>,
    #[serde(rename="com.edgedb.metadata.version")]
    version: Option<MajorVersion>,
    #[serde(rename="com.edgedb.metadata.current-version")]
    current_version: Option<Version<String>>,
    #[serde(rename="com.edgedb.metadata.start-conf")]
    start_conf: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
pub struct ContainerConfig {
    Labels: Option<ContainerLabels>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
pub struct Container {
    State: ContainerState,
    Config: ContainerConfig,
}

#[derive(Debug, Serialize)]
pub struct DockerMethod<'os, O: CurrentOs + ?Sized> {
    #[serde(skip)]
    os: &'os O,
    cli: PathBuf,
    #[serde(skip)]
    tags: Lazy<Vec<Tag>>,
}

#[derive(Debug, Clone)]
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

#[derive(Debug)]
pub struct Create<'a> {
    name: &'a str,
    image: &'a Image,
    port: u16,
    start_conf: StartConf,
}

pub struct DockerInstance<'a, O: CurrentOs + ?Sized> {
    method: &'a DockerMethod<'a, O>,
    name: String,
    container: Lazy<Option<Container>>,
    metadata: Lazy<Metadata>,
}

pub struct DockerRun {
    docker_cmd: PathBuf,
    name: String,
    command: Command,
}

pub struct DockerGuard {
    docker_cmd: PathBuf,
    name: String,
    child: Child,
}

impl fmt::Debug for DockerRun {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.command, f)
    }
}

impl DockerRun {
    fn new(docker_cmd: impl AsRef<Path>) -> DockerRun {
        let name = format!("edgedb_{}_{}", std::process::id(), timestamp());
        let mut command = Command::new(docker_cmd.as_ref());
        command.arg("run");
        command.arg("--rm");
        command.arg("--name").arg(&name);
        DockerRun {
            docker_cmd: docker_cmd.as_ref().to_path_buf(),
            name,
            command,
        }
    }
    fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Command {
        self.command.arg(arg)
    }
    fn run(&mut self) -> anyhow::Result<DockerGuard> {
        Ok(DockerGuard {
            docker_cmd: self.docker_cmd.clone(),
            name: self.name.clone(),
            child: self.command.spawn()?,
        })
    }
}

impl Drop for DockerGuard {
    fn drop(&mut self) {
        process::run(Command::new(&self.docker_cmd)
            .arg("stop")
            .arg(&self.name))
            .map_err(|e| {
                log::warn!("Error stopping container {:?}: {:#}",
                           self.name, e);
            }).ok();
         self.child.wait().map_err(|e| {
             log::error!("Error waiting for stopped container: {}", e);
         }).ok();
    }
}


fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
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
    fn full_version(&self) -> Version<String> {
        match self {
            Tag::Stable(v, rev) => Version(format!("{}-{}", v, rev)),
            Tag::Nightly(v) => Version(v.clone()),
        }
    }
    fn into_image(self) -> Image {
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
        }
    }
    pub fn into_distr(self) -> DistributionRef {
        self.into_image().into_ref()
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
        let docker_info_worked = cli.as_ref().map(|cli| {
            Command::new(cli)
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| {
                log::info!("Error running docker CLI: {}", e);
            })
            .map(|s| {
                if s.success() {
                    log::info!("Error running docker CLI: {}", s);
                }
                s.success()
            })
            .unwrap_or(false)
        }).unwrap_or(false);
        let supported = cli.is_some() && docker_info_worked;
        Ok(DockerCandidate {
            supported,
            platform_supported: cfg!(unix) || cfg!(windows),
            cli,
            docker_info_worked,
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
                Command-line tool: {cli}, docker info: {info}",
                cli=if self.cli.is_some() { "found" } else { "not found" },
                info=if self.docker_info_worked {
                    "okay"
                } else if self.cli.is_some() {
                    "failed"
                } else {
                    "skipped"
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
        match process::get_json_or_stderr::<Vec<_>>(&mut cmd)? {
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
        match process::get_json_or_stderr::<Vec<_>>(&mut cmd)? {
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
    fn minor_upgrade(&self, options: &Upgrade) -> anyhow::Result<()> {
        for inst in self._all_instances()? {
            if inst.get_version()?.is_nightly() {
                continue;
            }
            let version = inst.get_version()?;
            let version_query = version.to_query();
            let new = self._get_version(&version_query)
                .context("Unable to determine version")?;
            let old = inst.get_current_version()?;
            if !options.force {
                if let Some(old_ver) = &old {
                    if old_ver >= &new.version() {
                        log::info!(target: "edgedb::server::upgrade",
                            "Instance {} is up to date {}",
                            inst.name(), old_ver);
                        return Ok(());
                    }
                }
            }
            log::info!(target: "edgedb::server::upgrade",
                "Upgrading instance {}, version: {} to {}",
                inst.name(), version.title(), new.version());
            let create = Create {
                name: inst.name(),
                image: &new,
                port: inst.get_port()?,
                start_conf: inst.get_start_conf()?,
            };

            inst.delete()?;
            self.create(&create)?;
        }
        Ok(())
    }
    fn nightly_upgrade(&self, options: &Upgrade) -> anyhow::Result<()> {
        let version_query = VersionQuery::Nightly;
        let new = self.get_version(&version_query)
            .context("Unable to determine version")?;
        log::info!(target: "edgedb::server::upgrade",
            "Installing nightly {}", new.version());
        let new_version = new.version().clone();
        self.install(&install::Settings {
            method: self.name(),
            distribution: new,
            extra: LinkedHashMap::new(),
        })?;

        for inst in self._all_instances()? {
            if !inst.get_version()?.is_nightly() {
                continue;
            }

            let old = inst.get_current_version()?;
            let new = self._get_version(&version_query)
                .context("Unable to determine version")?;

            if !options.force {
                if let Some(old_ver) = old {
                    if old_ver >= &new_version {
                        log::info!(target: "edgedb::server::upgrade",
                            "Instance {} is up to date {}. Skipping.",
                            inst.name(), old_ver);
                        return Ok(());
                    }
                }
            }

            let dump_path = format!("./edgedb.upgrade.{}.dump", inst.name());
            upgrade::dump_and_stop(&inst, dump_path.as_ref())?;
            let meta = upgrade::UpgradeMeta {
                source: old.cloned().unwrap_or_else(|| Version("unknown".into())),
                target: new_version.clone(),
                started: SystemTime::now(),
                pid: std::process::id(),
            };
            self.reinit_and_restore(inst, &meta, dump_path.as_ref(), &new)?;
        }
        Ok(())
    }
    fn instance_upgrade(&self, name: &str,
        version_query: &Option<VersionQuery>,
        options: &Upgrade) -> anyhow::Result<()>
    {
        let inst = self._get_instance(name)?;
        let version_query = if let Some(q) = version_query {
            q
        } else if inst.get_version()?.is_nightly() {
            &VersionQuery::Nightly
        } else {
            &VersionQuery::Stable(None)
        };

        let new = self._get_version(&version_query)
            .context("Unable to determine version")?;
        let old = inst.get_current_version()?;
        let new_version = new.version().clone();
        let new_major = new.major_version().clone();
        let old_major = inst.get_version()?;

        if !options.force {
            if let Some(old_ver) = old {
                if old_ver >= &new_version {
                    log::info!(target: "edgedb::server::upgrade",
                        "Instance {} is up to date {}. Skipping.",
                        inst.name(), old_ver);
                    return Ok(());
                }
            }
        }

        log::info!(target: "edgedb::server::upgrade",
            "Installing version {}", new.version());
        self.install(&install::Settings {
            method: self.name(),
            distribution: new.clone().into_ref(),
            extra: LinkedHashMap::new(),
        })?;

        if !old_major.is_nightly() && &new_major == old_major {
            let create = Create {
                name: inst.name(),
                image: &new,
                port: inst.get_port()?,
                start_conf: inst.get_start_conf()?,
            };

            inst.delete()?;
            self.create(&create)?;
        } else {
            let dump_path = format!("./edgedb.upgrade.{}.dump", inst.name());
            upgrade::dump_and_stop(&inst, dump_path.as_ref())?;
            let meta = upgrade::UpgradeMeta {
                source: old.cloned()
                    .unwrap_or_else(|| Version("unknown".into())),
                target: new_version.clone(),
                started: SystemTime::now(),
                pid: std::process::id(),
            };
            self.reinit_and_restore(inst, &meta, dump_path.as_ref(), &new)?;
        }
        Ok(())
    }
    fn _all_instances<'x>(&'x self)
        -> anyhow::Result<Vec<DockerInstance<'x, O>>>
         where Self: 'os
    {
        let output = process::get_text(Command::new(&self.cli)
            .arg("volume")
            .arg("list")
            .arg("--filter")
            .arg(format!("label=com.edgedb.metadata.user={}",
                          whoami::username()))
            .arg("--format").arg("{{.Name}}"))?;
        let mut result = Vec::new();
        for volume in output.lines() {
            if let Some(name) = volume.strip_prefix("edgedb_") {
                result.push(DockerInstance {
                    name: name.into(),
                    method: self,
                    container: Lazy::lazy(),
                    metadata: Lazy::lazy(),
                });
            }
        }
        Ok(result)
    }
    fn _get_version(&self, query: &VersionQuery)
        -> anyhow::Result<Image>
    {
        let tag = self.get_tags()?
            .iter()
            .filter(|tag| tag.matches(query))
            .max()
            .with_context(|| format!("version {} not found", query))?;
        Ok(tag.clone().into_image())
    }
    fn _get_instance<'x>(&'x self, name: &str)
        -> anyhow::Result<DockerInstance<'x, O>>
    {
        let volume = format!("edgedb_{}", name);
        match self.inspect_volume(&volume)? {
            Some(_) => {
                Ok(DockerInstance {
                    name: name.into(),
                    method: self,
                    container: Lazy::lazy(),
                    metadata: Lazy::lazy(),
                })
            }
            None => {
                Err(InstanceNotFound(
                    anyhow::anyhow!("No docker volume {:?} found", volume)
                ).into())
            }
        }
    }
    fn create(&self, options: &Create) -> anyhow::Result<()> {
        let mut cmd = Command::new(&self.cli);
        cmd.arg("container");
        match options.start_conf {
            StartConf::Auto => {
                cmd.arg("run");
                cmd.arg("--detach");
                cmd.arg("--restart=always");
            }
            StartConf::Manual => {
                cmd.arg("create");
            }
        }
        let container_name = format!("edgedb_{}", options.name);
        let volume_name = &container_name;
        cmd.arg("--name").arg(&container_name);
        cmd.arg("--user=999:999");
        cmd.arg(format!("--publish={0}:{0}", options.port));
        cmd.arg("--mount")
           .arg(format!("source={},target=/var/lib/edgedb/data",
                        volume_name));
        cmd.arg(format!("--label=com.edgedb.metadata.user={}",
                        whoami::username()));
        cmd.arg(format!("--label=com.edgedb.metadata.version={}",
                        options.image.major_version.title()));
        cmd.arg(format!("--label=com.edgedb.metadata.current-version={}",
                        options.image.version));
        cmd.arg(format!("--label=com.edgedb.metadata.port={}",
                        options.port));
        cmd.arg(format!("--label=com.edgedb.metadata.start-conf={}",
                        options.start_conf));
        cmd.arg(options.image.tag.as_image_name());
        cmd.arg("edgedb-server");
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
            .arg(format!("/var/lib/edgedb/data/{}", options.name));
        cmd.arg("--port").arg(options.port.to_string());
        cmd.arg("--bind-address=0.0.0.0");
        process::run(&mut cmd).with_context(|| {
            match options.start_conf {
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
    fn delete_container(&self, name: &str) -> anyhow::Result<bool> {
        match process::run_or_stderr(Command::new(&self.cli)
            .arg("container")
            .arg("stop")
            .arg(name))?
        {
            Ok(_) => {}
            Err(text) if text.contains("No such container") => return Ok(false),
            Err(text) => anyhow::bail!("docker error: {}", text),
        }
        match process::run_or_stderr(Command::new(&self.cli)
            .arg("container")
            .arg("rm")
            .arg(name))?
        {
            Ok(_) => {}
            Err(text) if text.contains("No such container") => return Ok(false),
            Err(text) => anyhow::bail!("docker error: {}", text),
        }
        Ok(true)
    }
    #[context("failed to restore {:?}", inst.name())]
    fn reinit_and_restore(&self, inst: DockerInstance<O>,
        meta: &upgrade::UpgradeMeta, dump_path: &Path, new_image: &Image)
        -> anyhow::Result<()>
    {
        let volume = inst.volume_name();
        let port = inst.get_port()?;

        let upgrade_container = format!("edgedb_upgrade_{}", inst.name());
        if self.inspect_container(&upgrade_container)?.is_some() {
            anyhow::bail!("upgrade is already in progress");
        }
        process::run(
            Command::new(&inst.method.cli)
            .arg("container")
            .arg("create")
            .arg("--name").arg(&upgrade_container)
            .arg("--label").arg(format!("com.edgedb.upgrade-in-progress={}",
                serde_json::to_string(&meta).unwrap()))
            .arg("busybox")
            .arg("true")
        )?;

        process::run(
            Command::new(&inst.method.cli)
            .arg("container")
            .arg("run")
            .arg("--rm")
            .arg("--user=999:999")
            .arg("--mount")
                .arg(format!("source={},target=/mnt", volume))
            .arg("busybox")
            .arg("sh")
            .arg("-ec")
            .arg(format!(r###"
                    rm -rf /mnt/{name}.backup
                    mv /mnt/{name} /mnt/{name}.backup
                    echo {backup_meta} > /mnt/{name}.backup/backup.json
                "###,
                name=inst.name(),
                backup_meta=shell_words::quote(&
                    serde_json::to_string(&upgrade::BackupMeta {
                        timestamp: SystemTime::now(),
                    })?),
            ))
        )?;
        self._reinit_and_restore(
            &inst, port, &volume, new_image, dump_path, &upgrade_container,
        ).map_err(|e| {
            eprintln!("edgedb error: failed to restore {:?}: {}",
                      inst.name(), e);
            eprintln!("To undo run:\n  edgedb server revert {:?}",
                      inst.name());
            ExitCode::new(1).into()
        })
    }

    fn _reinit_and_restore(&self,  inst: &DockerInstance<O>, port: u16,
        volume: &str, new_image: &Image, dump_path: &Path,
        upgrade_container: &str)
        -> anyhow::Result<()>
    {
        let tmp_role = format!("tmp_upgrade_{}", timestamp());
        let tmp_password = generate_password();

        let mut cmd = DockerRun::new(&inst.method.cli);
        cmd.arg("--user=999:999");
        cmd.arg(format!("--publish={0}:{0}", port));
        cmd.arg("--mount")
           .arg(format!("source={},target=/var/lib/edgedb/data", volume));
        cmd.arg(new_image.tag.as_image_name());
        cmd.arg("edgedb-server");
        cmd.arg("--bootstrap-command")
            .arg(format!(r###"
                CREATE SUPERUSER ROLE {role} {{
                    SET password := {password};
                }};
            "###, role=tmp_role, password=quote_string(&tmp_password)));
        cmd.arg("--log-level=warn");
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", inst.name()));
        cmd.arg("--port").arg(port.to_string());
        cmd.arg("--bind-address=0.0.0.0");
        log::debug!("Running server: {:?}", cmd);
        let child = cmd.run()
            .with_context(|| format!("error running server {:?}", cmd))?;

        let mut params = inst.get_connector(false)?;
        params.user(&tmp_role);
        params.password(&tmp_password);
        params.database("edgedb");
        task::block_on(
            upgrade::restore_instance(inst, &dump_path, params.clone())
        )?;
        let mut conn_params = inst.get_connector(false)?;
        conn_params.wait_until_available(Duration::from_secs(30));
        task::block_on(async {
            let mut cli = conn_params.connect().await?;
            cli.execute(&format!(r###"
                DROP ROLE {};
            "###, role=tmp_role)).await?;
            Ok::<(), anyhow::Error>(())
        })?;
        log::info!(target: "edgedb::server::upgrade",
            "Restarting instance {:?} to apply changes from `restore --all`",
            &inst.name());
        drop(child);

        let method = inst.method;
        let create = Create {
            name: inst.name(),
            image: new_image,
            port,
            start_conf: inst.get_start_conf()?,
        };
        inst.delete()?;
        method.create(&create)?;

        process::run(
            Command::new(&inst.method.cli)
            .arg("container")
            .arg("rm")
            .arg(&upgrade_container)
        )?;

        Ok(())
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
    fn uninstall(&self, distr: &DistributionRef) -> anyhow::Result<()> {
        let image = distr.downcast_ref::<Image>()
            .context("invalid distribution for Docker")?;
        match process::run_or_stderr(Command::new(&self.cli)
            .arg("image")
            .arg("rm")
            .arg(image.tag.as_image_name()))?
        {
            Ok(_) => {}
            Err(text) if text.contains("No such image") => {},
            Err(text) => anyhow::bail!("docker error: {}", text),
        }
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
        Ok(self._get_version(query)?.into_ref())
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
        assert!(!system);
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
        let user = whoami::username();
        let md = serde_json::to_string(&settings.metadata())?;
        process::get_text(Command::new(&self.cli)
            .arg("volume")
            .arg("create")
            .arg(volume)
            .arg(format!("--label=com.edgedb.metadata.user={}", user)))?;

        process::run(Command::new(&self.cli)
            .arg("run")
            .arg("--rm")
            .arg("--mount").arg(format!("source={},target=/mnt", volume))
            .arg(image.tag.as_image_name())
            .arg("sh")
            .arg("-c")
            .arg(format!("chown -R 999:999 /mnt")))?;

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
        cmd.arg("--bootstrap-command")
            .arg(bootstrap_script(settings, &password));
        cmd.arg("--log-level=warn");
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", settings.name));

        log::debug!("Running bootstrap {:?}", cmd);
        match cmd.status() {
            Ok(s) if s.success() => {}
            Ok(s) => anyhow::bail!("Command {:?} {}", cmd, s),
            Err(e) => Err(e).context(format!("Failed running {:?}", cmd))?,
        }

        process::run(Command::new(&self.cli)
            .arg("run")
            .arg("--rm")
            .arg("--mount").arg(format!("source={},target=/mnt", volume))
            .arg(image.tag.as_image_name())
            .arg("sh")
            .arg("-c")
            .arg(format!(r###"
                    echo {metadata} > /mnt/{name}/metadata.json
                "###,
                name=settings.name,
                metadata=shell_words::quote(&md),
            )))?;

        save_credentials(&settings, &password)?;
        drop(password);

        self.create(&Create {
            name: &settings.name,
            image: &image,
            port: settings.port,
            start_conf: settings.start_conf,
        })?;
        if !settings.suppress_messages {
            println!("To connect run:\n  edgedb -I {}",
                     settings.name.escape_default());
        }

        Ok(())
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>>
         where Self: 'os
    {
        let output = process::get_text(Command::new(&self.cli)
            .arg("volume")
            .arg("list")
            .arg("--filter")
            .arg(format!("label=com.edgedb.metadata.user={}",
                          whoami::username()))
            .arg("--format").arg("{{.Name}}"))?;
        let mut result = Vec::new();
        for volume in output.lines() {
            if let Some(name) = volume.strip_prefix("edgedb_") {
                result.push(DockerInstance {
                    name: name.into(),
                    method: self,
                    container: Lazy::lazy(),
                    metadata: Lazy::lazy(),
                }.into_ref());
            }
        }
        Ok(result)
    }
    fn get_instance<'x>(&'x self, name: &str)
        -> anyhow::Result<InstanceRef<'x>>
    {
        self._get_instance(name).map(|inst| inst.into_ref())
    }
    fn upgrade(&self, todo: &upgrade::ToDo, options: &Upgrade)
        -> anyhow::Result<()>
    {
        use upgrade::ToDo::*;

        match todo {
            MinorUpgrade => {
                self.minor_upgrade(options)
            }
            NightlyUpgrade => {
                self.nightly_upgrade(options)
            }
            InstanceUpgrade(name, ref version) => {
                self.instance_upgrade(name, version, options)
            }
        }
    }
    fn destroy(&self, options: &Destroy) -> anyhow::Result<()> {
        let mut found = false;
        let container_name = format!("edgedb_{}", options.name);
        if self.delete_container(&container_name)? {
            log::info!(target: "edgedb::server::destroy",
                "Removed container {:?}", container_name);
            found = true;
        }
        let up_container = format!("edgedb_upgrade_{}", options.name);
        if self.delete_container(&up_container)? {
            log::info!(target: "edgedb::server::destroy",
                "Removed container {:?}", up_container);
            found = true;
        }
        match process::run_or_stderr(Command::new(&self.cli)
            .arg("volume")
            .arg("remove")
            .arg(&container_name))?
        {
            Ok(_) => {
                log::info!(target: "edgedb::server::destroy",
                    "Removed volume {:?}", container_name);
                found = true;
            }
            Err(text) if text.contains("No such volume") => {},
            Err(text) => anyhow::bail!("docker error: {}", text),
        }
        let credentials = credentials::path(&options.name)?;
        if credentials.exists() {
            found = true;
            log::info!(target: "edgedb::server::destroy",
                "Removing credentials file {}", credentials.display());
            fs::remove_file(&credentials)?;
        }
        if found {
            Ok(())
        } else {
            Err(InstanceNotFound(anyhow::anyhow!(
                "no instance {:?} found", options.name)).into())
        }
    }
}

impl<O: CurrentOs + ?Sized> DockerInstance<'_, O> {
    fn get_metadata(&self) -> anyhow::Result<Metadata> {
        Ok(Metadata {
            version: self.get_version()?.clone(),
            slot: None,
            current_version: None, // TODO
            method: InstallMethod::Docker,
            port: self.get_port()?,
            start_conf: self.get_start_conf()?,
        })
    }
    fn get_backup(&self) -> anyhow::Result<BackupStatus> {
        let volume_name = self.volume_name();
        let mut cmd = Command::new(&self.method.cli);
        cmd.arg("run");
        cmd.arg("--rm");
        cmd.arg("--mount").arg(format!("source={},target=/mnt", volume_name));
        cmd.arg("busybox");
        cmd.arg("cat");
        cmd.arg(format!("/mnt/{}.backup/backup.json", self.name));
        cmd.arg(format!("/mnt/{}.backup/metadata.json", self.name));
        let out = cmd.output()
            .with_context(|| format!("error running {:?}", cmd))?;
        if out.status.success() {
            let mut items = serde_json::Deserializer::from_slice(&out.stdout)
                .into_iter::<serde_json::Value>();
            let backup_meta = decode_next(&mut items)
                .context("error reading backup.json").into();
            let data_meta = decode_next(&mut items)
                .context("error reading metadata.json").into();
            Ok(BackupStatus::Exists {
                backup_meta,
                data_meta,
            })
        } else {
            let stderr = String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("can decode error output of {:?}", cmd)
                })?;
            if stderr.contains("No such file or directory") {
                return Ok(BackupStatus::Absent);
            }
            anyhow::bail!(stderr);
        }
    }
    fn get_meta(&self) -> anyhow::Result<&Metadata> {
        self.metadata.get_or_try_init(|| {
            let volume_name = self.volume_name();
            let data = process::get_text(Command::new(&self.method.cli)
                .arg("run")
                .arg("--rm")
                .arg("--mount")
                    .arg(format!("source={},target=/mnt", volume_name))
                .arg("busybox")
                .arg("cat")
                .arg(format!("/mnt/{}/metadata.json", self.name)))?;
            Ok(serde_json::from_str(&data)?)
        })
    }
    fn get_container(&self) -> anyhow::Result<&Option<Container>> {
        self.container.get_or_try_init(|| {
            self.method.inspect_container(&self.volume_name())
        })
    }
    fn get_labels(&self) -> anyhow::Result<Option<&ContainerLabels>> {
        Ok(self.get_container()?.as_ref()
            .and_then(|c| c.Config.Labels.as_ref()))
    }
    fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>> {
        Ok(self.get_labels()?
            .and_then(|labels| labels.current_version.as_ref()))
    }
    fn container_name(&self) -> String {
        format!("edgedb_{}", self.name)
    }
    fn volume_name(&self) -> String {
        format!("edgedb_{}", self.name)
    }
    fn delete(&self) -> anyhow::Result<()> {
        self.method.delete_container(&self.container_name())?;
        Ok(())
    }

}

impl<O: CurrentOs + ?Sized> Instance for DockerInstance<'_, O> {
    fn name(&self) -> &str {
        &self.name
    }
    fn method(&self) -> &dyn Method {
        self.method
    }
    fn get_version(&self) -> anyhow::Result<&MajorVersion> {
        if let Some(ver) = self.get_labels()?
            .and_then(|labels| labels.version.as_ref())
        {
            return Ok(ver)
        }
        Ok(&self.get_meta()?.version)
    }
    fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>> {
        if let Some(ver) = self.get_labels()?
            .and_then(|labels| labels.current_version.as_ref())
        {
            return Ok(Some(ver))
        }
        Ok(self.get_meta()?.current_version.as_ref())
    }
    fn get_port(&self) -> anyhow::Result<u16> {
        if let Some(port) = self.get_labels()?
            .and_then(|labels| labels.port.as_ref())
            .and_then(|port| port.parse().ok())
        {
            return Ok(port)
        }
        Ok(self.get_meta()?.port)
    }
    fn get_start_conf(&self) -> anyhow::Result<StartConf> {
        if let Some(start_conf) = self.get_labels()?
            .and_then(|labels| labels.start_conf.as_ref())
            .and_then(|start_conf| start_conf.parse().ok())
        {
            return Ok(start_conf)
        }
        Ok(self.get_meta()?.start_conf)
    }
    fn get_status(&self) -> Status {
        use DataDirectory::*;
        use Service::*;

        let container_name = self.container_name();
        let storage = Storage::DockerVolume(self.volume_name());
        let volume_exists = self.method.storage_exists(&storage)
            .unwrap_or(false);
        let (service, service_exists) = match self.get_container() {
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
        let up_container = format!("edgedb_upgrade_{}", self.name);
        let metadata = if volume_exists {
            self.get_metadata()
        } else {
            Err(anyhow::anyhow!("no volume named {:?}", container_name))
        };
        let data_status = match self.method.inspect_container(&up_container) {
            Ok(Some(c)) => {
                Upgrading(c.Config.Labels.as_ref()
                    .and_then(|x| x.upgrading.as_ref())
                    .ok_or_else(|| anyhow::anyhow!("no upgrade metadata"))
                    .and_then(|s| {
                        Ok(serde_json::from_str(s)
                            .context("cannot decode upgrade metadata")?)
                    }))
            }
            Ok(None) => match metadata {
                Ok(_) => Normal,
                Err(_) if volume_exists => NoMetadata,
                Err(_) => Absent,
            },
            Err(_) => Absent,
        };
        let reserved_port =
            // TODO(tailhook) cache ports
            read_ports()
            .map_err(|e| log::warn!("{:#}", e))
            .ok()
            .and_then(|ports| ports.get(&self.name).cloned());
        let port_status = probe_port(&metadata, &reserved_port);
        let backup = match self.get_backup() {
            Ok(v) => v,
            Err(e) => BackupStatus::Error(e.into()),
        };
        let credentials_file_exists = home_dir().map(|home| {
            home.join(".edgedb")
                .join("credentials")
                .join(format!("{}.json", self.name))
                .exists()
        }).unwrap_or(false);

        Status {
            method: InstallMethod::Docker,
            name: self.name.clone(),
            service,
            metadata,
            reserved_port,
            port_status,
            storage,
            data_status,
            backup,
            service_exists,
            credentials_file_exists,
        }
    }
    fn start(&self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            process::run(Command::new(&self.method.cli)
                .arg("container")
                .arg("start")
                .arg("--attach")
                .arg("--interactive")
                .arg(self.container_name()))?;
        } else {
            process::run(Command::new(&self.method.cli)
                .arg("container")
                .arg("start")
                .arg(self.container_name()))?;
        }
        Ok(())
    }
    fn stop(&self, _options: &Stop) -> anyhow::Result<()> {
        process::run(Command::new(&self.method.cli)
            .arg("container")
            .arg("stop")
            .arg(self.container_name()))?;
        Ok(())
    }
    fn restart(&self, _options: &Restart) -> anyhow::Result<()> {
        process::run(Command::new(&self.method.cli)
            .arg("container")
            .arg("restart")
            .arg(self.container_name()))?;
        Ok(())
    }
    fn logs(&self, options: &Logs) -> anyhow::Result<()> {
        let mut cmd = Command::new(&self.method.cli);
        cmd.arg("container");
        cmd.arg("logs");
        cmd.arg(self.container_name());
        if let Some(n) = options.tail {
            cmd.arg(format!("--tail={}", n));
        }
        if options.follow {
            cmd.arg("--follow");
        }
        process::run(&mut cmd)
    }
    fn service_status(&self) -> anyhow::Result<()> {
        process::run(Command::new(&self.method.cli)
            .arg("container")
            .arg("inspect")
            .arg(self.container_name()))?;
        Ok(())
    }
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        if admin {
            anyhow::bail!("Cannot connect to admin socket in docker")
        } else {
            get_connector(self.name())
        }
    }
    fn get_command(&self) -> anyhow::Result<Command> {
        anyhow::bail!("no get_command is supported for docker instances");
    }
    fn upgrade<'x>(&'x self, meta: &Metadata)
        -> anyhow::Result<InstanceRef<'x>>
    {
        Ok(DockerInstance {
            method: self.method,
            name: self.name.clone(),
            container: Lazy::lazy(),
            metadata: Lazy::eager(meta.clone()),
        }.into_ref())
    }
    fn revert(&self, metadata: &Metadata) -> anyhow::Result<()> {
        let name = self.name();
        let volume = self.volume_name();

        let current_version = metadata.current_version.as_ref()
            .ok_or_else(|| anyhow::anyhow!("broken metadata, \
                no `com.edgedb.metadata.current-version` label"))?;
        let tag = self.method.get_tags()?
            .iter()
            .filter(|tag| &tag.full_version() == current_version)
            .max()
            .with_context(|| format!("version {} not found", current_version))?;
        let image = tag.clone().into_image();
        let create = Create {
            name: name,
            image: &image,
            port: self.get_port()?,
            start_conf: self.get_start_conf()?,
        };

        self.stop(&Stop { name: name.into() })?;
        process::run(
            Command::new(&self.method.cli)
            .arg("container")
            .arg("run")
            .arg("--rm")
            .arg("--user=999:999")
            .arg("--mount")
                .arg(format!("source={},target=/mnt", volume))
            .arg("busybox")
            .arg("sh")
            .arg("-ec")
            .arg(format!(r###"
                    rm -rf /mnt/{name}
                    mv /mnt/{name}.backup /mnt/{name}
                    rm /mnt/{name}/backup.json
                "###,
                name=name,
            ))
        )?;

        self.delete()?;
        self.method.create(&create)?;

        let upgrade_container = format!("edgedb_upgrade_{}", name);
        self.method.delete_container(&upgrade_container)?;

        Ok(())
    }
}

impl<O: CurrentOs + ?Sized> fmt::Debug for DockerInstance<'_, O> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let DockerInstance {
            name,
            method: _method,
            metadata: _,
            container: _,
        } = self;
        f.debug_struct("DockerInstance")
            .field("name", name)
            .finish()
    }
}

fn decode_next<'x, R, T>(
    iter: &mut serde_json::StreamDeserializer::<'x, R, serde_json::Value>)
    -> anyhow::Result<T>
    where R: serde_json::de::Read<'x>,
          T: serde::de::DeserializeOwned,
{
    let item = iter.next().ok_or_else(|| anyhow::anyhow!("no data"))??;
    Ok(serde_json::from_value(item)?)
}


