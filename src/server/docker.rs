use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, Duration, UNIX_EPOCH};

use anyhow::Context;
use async_std::task;
use edgedb_client as client;
use edgeql_parser::helpers::{quote_string, quote_name};
use fn_error_context::context;
use serde::{Serialize, Deserialize};

use crate::credentials::{self, get_connector};
use crate::process;

use crate::commands::ExitCode;
use crate::print;
use crate::server::create::{bootstrap_script, save_credentials};
use crate::server::create::{self as create_mod, read_ports, Storage};
use crate::server::detect::Lazy;
use crate::server::distribution::{DistributionRef, Distribution};
use crate::server::errors::InstanceNotFound;
use crate::server::install;
use crate::server::metadata::Metadata;
use crate::server::methods::InstallMethod;
use crate::server::options::{Start, Stop, Restart, Upgrade, Destroy, Logs};
use crate::server::options::{StartConf};
use crate::server::os_trait::{CurrentOs, Method, Instance, InstanceRef};
use crate::server::remote;
use crate::server::reset_password::{generate_password};
use crate::server::status::{Service, Status, DataDirectory, BackupStatus};
use crate::server::status::{probe_port};
use crate::server::unix;
use crate::server::upgrade;
use crate::server::version::{Version, VersionSlot, VersionQuery, VersionMarker};


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
    Nightly(String, String),
}

#[derive(Debug, Deserialize)]
pub struct TagList {
    count: u64,
    next: Option<String>,
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
    version: Option<VersionMarker>,
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
    Image: String,
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
    version_slot: VersionSlot,
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

fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

impl Tag {
    pub fn matches(&self, q: &VersionQuery) -> bool {
        match (self, q) {
            (Tag::Stable(_, _), VersionQuery::Stable(None)) => true,
            (Tag::Stable(t, _), VersionQuery::Stable(Some(q))) => t == q.num(),
            (Tag::Nightly(..), VersionQuery::Nightly) => true,
            _ => false,
        }
    }
    fn full_version(&self) -> Version<String> {
        match self {
            Tag::Stable(v, rev) => Version(format!("{}-{}", v, rev)),
            Tag::Nightly(slot, cv) => Version(format!("{}_{}", slot, cv)),
        }
    }
    fn into_image(self) -> Image {
        match &self {
            Tag::Stable(v, rev) => Image {
                version_slot: VersionSlot::Stable(Version(v.clone())),
                version: Version(format!("{}-{}", v, rev)),
                tag: self,
            },
            Tag::Nightly(slot, cv) => Image {
                version_slot: VersionSlot::Nightly(Version(slot.clone())),
                version: Version(format!("{}_{}", slot, cv)),
                tag: self,
            },
        }
    }
    pub fn into_distr(self) -> DistributionRef {
        self.into_image().into_ref()
    }
    pub fn as_image_name(&self) -> String {
        match &self {
            Tag::Stable(v, _) => format!("edgedb/edgedb:{}", v),
            Tag::Nightly(slot, cv) =>
                format!("edgedb/edgedb:nightly_{}_{}", slot, cv),
        }
    }
}

impl Distribution for Image {
    fn version_slot(&self) -> &VersionSlot {
        &self.version_slot
    }
    fn version(&self) -> &Version<String> {
        &self.version
    }
}

#[cfg(target_os="macos")]
fn platform_supported() -> bool {
    let mut utsname = libc::utsname {
        sysname: [0; 256],
        nodename: [0; 256],
        release: [0; 256],
        version: [0; 256],
        machine: [0; 256],
    };
    if unsafe { libc::uname(&mut utsname) } == 1 {
        log::warn!("Cannot get uname: {}", std::io::Error::last_os_error());
        return false;
    }
    let machine: &[u8] = unsafe { std::mem::transmute(&utsname.machine[..]) };
    let mend: usize = machine.iter().position(|&b| b == 0).unwrap_or(256);
    match std::str::from_utf8(&machine[..mend]) {
        Ok(machine) => {
            log::debug!("Architecture {:?}", machine);
            return machine == "x86_64";
        }
        Err(e) => {
            log::warn!("Cannot decode machine from uname: {}", e);
            return false;
        }
    }
}

#[cfg(any(all(unix, not(target_os="macos")), windows))]
fn platform_supported() -> bool {
    true
}

impl DockerCandidate {
    pub fn detect() -> anyhow::Result<DockerCandidate> {
        let cli = which::which("docker").ok();
        let docker_info_worked = cli.as_ref().map(|cli| {
            process::Native::new("docker info", "docker", cli)
                .arg("info")
                .status()
                .map_err(|e| {
                    log::info!("Error running docker CLI: {}", e);
                })
                .map(|s| {
                    if !s.success() {
                        log::info!("Error running docker CLI: {}", s);
                    }
                    s.success()
                })
                .unwrap_or(false)
        }).unwrap_or(false);
        let platform_supported = platform_supported();
        let supported = platform_supported &&
            cli.is_some() && docker_info_worked;
        Ok(DockerCandidate {
            supported,
            platform_supported: platform_supported,
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
        } else if let Some(version) = name.strip_prefix("nightly_") {
            // example: `nightly_1-beta3-dev202107130000_cv202107130000`
            let (slot, cv) = version.split_once('_')
                    .unwrap_or((version, ""));
            return Some(Tag::Nightly(slot.into(), cv.into()));
        } else {
            // maybe `latest` or something unrelated
            None
        }
    }
}

impl<'os, O: CurrentOs + ?Sized> DockerMethod<'os, O> {
    fn docker_run(&self, description: &'static str,
        image: impl Into<Cow<'static, str>>,
        cmd: impl Into<Cow<'static, str>>)
        -> process::Docker
    {
        process::Docker::new(description, &self.cli, image, cmd)
    }
    fn get_tags(&self) -> anyhow::Result<&[Tag]> {
        self.tags.get_or_try_init(|| {
            task::block_on(async {
                let mut url = "https://hub.docker.com/\
                        v2/repositories/edgedb/edgedb/tags\
                        ?page_size=1000".to_string();
                let mut tags = Vec::new();
                loop {
                    let data: TagList = remote::get_json(&url,
                        "failed to fetch tag list from the Docker registry"
                    ).await?;
                    tags.extend(data.results
                        .into_iter()
                        .flat_map(|t| {
                            t.images.get(0).and_then(|img| {
                                Tag::from_pair(&t.name, &img.digest)
                            })
                        }));
                    if let Some(next_url) = data.next {
                        url = next_url;
                    } else {
                        break;
                    };
                }
                Ok(tags)
            })
        }).map(|v| &v[..])
    }
    pub fn inspect_container(&self, name: &str)
        -> anyhow::Result<Option<Container>>
    {
        let mut cmd = process::Native::new(
            "container inspect", "docker", &self.cli);
        cmd.arg("container");
        cmd.arg("inspect");
        cmd.arg(name);
        match cmd.get_json_or_stderr::<Vec<_>>()? {
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
        let mut cmd = process::Native::new(
            "volume inspect", "docker", &self.cli);
        cmd.arg("volume");
        cmd.arg("inspect");
        cmd.arg(name);
        match cmd.get_json_or_stderr::<Vec<_>>()? {
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
    fn minor_upgrade(&self, options: &Upgrade) -> anyhow::Result<bool> {
        let mut rv = false;
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
                        return Ok(rv);
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

            rv = true;
            inst.delete()?;
            self.create(&create)?;
        }
        Ok(rv)
    }
    fn instance_upgrade(&self, name: &str,
        version_query: &Option<VersionQuery>,
        options: &Upgrade) -> anyhow::Result<bool>
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
        let new_major = new.version_slot().to_marker();
        let old_major = inst.get_version()?;

        if !options.force {
            if old_major == &new_major {
                if let Some(old_ver) = old {
                    // old nightly versions had neither `-` nor `.` in the name,
                    // so just consider them old
                    if old_ver.num().contains(|c| c == '-' || c == '.') &&
                        old_ver >= &new_version
                    {
                        log::info!(target: "edgedb::server::upgrade",
                            "Instance {} is up to date {}. Skipping.",
                            inst.name(), old_ver);
                        return Ok(false);
                    }
                }
            }
        }

        log::info!(target: "edgedb::server::upgrade",
            "Installing version {}", new.version());
        self.install(&install::Settings {
            method: self.name(),
            distribution: new.clone().into_ref(),
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
        Ok(true)
    }
    fn _all_instances<'x>(&'x self)
        -> anyhow::Result<Vec<DockerInstance<'x, O>>>
         where Self: 'os
    {
        let output = process::Native::new("volume list", "docker", &self.cli)
            .arg("volume")
            .arg("list")
            .arg("--filter")
            .arg(format!("label=com.edgedb.metadata.user={}",
                          whoami::username()))
            .arg("--format").arg("{{.Name}}")
            .get_stdout_text()?;
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
        let mut cmd = process::Native::new("container", "docker", &self.cli);
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
                        options.image.version_slot.title()));
        cmd.arg(format!("--label=com.edgedb.metadata.current-version={}",
                        options.image.version));
        cmd.arg(format!("--label=com.edgedb.metadata.port={}",
                        options.port));
        cmd.arg(format!("--label=com.edgedb.metadata.start-conf={}",
                        options.start_conf));
        cmd.env("EDGEDB_SERVER_INSTANCE_NAME", options.name);
        cmd.arg("--env").arg("EDGEDB_SERVER_INSTANCE_NAME");
        cmd.arg("--env").arg("EDGEDB_SERVER_ALLOW_INSECURE_HTTP_CLIENTS=1");
        cmd.arg("--env").arg("EDGEDB_SERVER_DOCKER_LOG_LEVEL=warning");
        cmd.arg(options.image.tag.as_image_name());
        cmd.arg("edgedb-server");
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
            .arg(format!("/var/lib/edgedb/data/{}", options.name));
        cmd.arg("--port").arg(options.port.to_string());
        cmd.arg("--bind-address=0.0.0.0");
        cmd.run().with_context(|| {
            match options.start_conf {
                StartConf::Auto => "starting server",
                StartConf::Manual => "creating container",
            }
        })?;
        Ok(())
    }
    fn delete_container(&self, name: &str) -> anyhow::Result<bool> {
        match process::Native::new("container stop", "docker", &self.cli)
            .arg("container")
            .arg("stop")
            .arg(name)
            .run_or_stderr()?
        {
            Ok(_) => {}
            Err((_, text)) if text.contains("No such container")
                => return Ok(false),
            Err((s, text)) => anyhow::bail!("docker error: {}, {}", s, text),
        }
        match process::Native::new("container remove", "docker", &self.cli)
            .arg("container")
            .arg("rm")
            .arg(name)
            .run_or_stderr()?
        {
            Ok(_) => {}
            Err((_, text)) if text.contains("No such container")
                => return Ok(false),
            Err((s, text)) => anyhow::bail!("docker error: {}: {}", s, text),
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

        process::Native::new("marker container", "docker", &inst.method.cli)
            .arg("container")
            .arg("create")
            .arg("--name").arg(&upgrade_container)
            .arg("--label").arg(format!("com.edgedb.upgrade-in-progress={}",
                serde_json::to_string(&meta).unwrap()))
            .arg("busybox")
            .arg("true")
            .run()?;

        inst.method.docker_run("meta backup", "busybox", "sh")
            .mount(&volume, "/mnt")
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
            .run()?;

        self._reinit_and_restore(
            &inst, port, &volume, new_image, dump_path, &upgrade_container,
        ).map_err(|e| {
            print::error(
                format!("failed to restore {:?}: {}", inst.name(), e),
            );
            eprintln!("To undo, run:\n  edgedb instance revert {:?}",
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

        let cert_required =
            new_image.version_slot.slot_name() >= &Version("1-beta3");
        let mut cmd = self.docker_run("bootstrap",
            new_image.tag.as_image_name(), "edgedb-server");
        if cert_required {
            cmd.env("EDGEDB_HIDE_GENERATED_CERT", "1");
        }
        cmd.env_default("EDGEDB_SERVER_LOG_LEVEL", "warn");
        cmd.env_default("EDGEDB_SERVER_DOCKER_LOG_LEVEL", "warning");
        cmd.expose_port(port);
        cmd.mount(volume, "/var/lib/edgedb/data");
        cmd.arg("--bootstrap-command");
        cmd.arg(format!(r###"
            CREATE SUPERUSER ROLE {role} {{
                SET password := {password};
            }};
        "###, role=tmp_role, password=quote_string(&tmp_password)));
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir");
        cmd.arg(format!("/var/lib/edgedb/data/{}", inst.name()));
        cmd.arg("--bootstrap-only");
        if cert_required {
            cmd.arg("--generate-self-signed-cert");
        }
        cmd.run()?;

        if cert_required {
            let image = new_image.tag.as_image_name();
            let output = self.docker_run("read cert", image, "sh")
                .as_root()
                .mount(volume, "/mnt")
                .arg("-c")
                .arg(format!(r###"
                    cp /mnt/{name}.backup/*.pem /mnt/{name}/ || \
                    cat /mnt/{name}/edbtlscert.pem
                "###, name=inst.name()))
                .get_stdout_text()?;

            // If the certificates existed before the upgrade, the command
            // above won't output anything and we will skip updating below.
            let cert_data = output.find("-----BEGIN CERTIFICATE-----")
                .zip(find_end(&output, "-----END CERTIFICATE-----"))
                .map(|(start, end)| &output[start..end]);

            if let Some(cert) = cert_data {
                if let Err(e) = credentials::add_certificate(
                    inst.name(), &cert
                ) {
                    log::warn!("Could not update credentials file: {:#}", e);
                }
            }
        }

        let mut cmd = inst.method.docker_run("server",
            new_image.tag.as_image_name(), "edgedb-server");
        cmd.env_default("EDGEDB_SERVER_LOG_LEVEL", "warn");
        cmd.env_default("EDGEDB_SERVER_DOCKER_LOG_LEVEL", "warning");
        cmd.expose_port(port);
        cmd.mount(volume, "/var/lib/edgedb/data");
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", inst.name()));
        cmd.arg("--port").arg(port.to_string());
        cmd.arg("--bind-address=0.0.0.0");
        cmd.background_for(async {
            let mut params = inst.get_connector(false)?;
            params.user(&tmp_role);
            params.password(&tmp_password);
            params.database("edgedb");
            upgrade::restore_instance(inst, &dump_path, params.clone()).await?;

            let mut conn_params = inst.get_connector(false)?;
            conn_params.wait_until_available(Duration::from_secs(30));
            let mut cli = conn_params.connect().await?;
            cli.execute(&format!(r###"
                DROP ROLE {};
            "###, role=tmp_role)).await?;

            log::info!(target: "edgedb::server::upgrade",
                "Restarting instance {:?} to apply changes \
                from `restore --all`",
                &inst.name());

            Ok(())
        })?;

        let method = inst.method;
        let create = Create {
            name: inst.name(),
            image: new_image,
            port,
            start_conf: inst.get_start_conf()?,
        };
        inst.delete()?;
        method.create(&create)?;

        process::Native::new("container rm", "docker", &inst.method.cli)
            .arg("container")
            .arg("rm")
            .arg(&upgrade_container)
            .run()?;

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
        process::Native::new("image pull", "docker", &self.cli)
            .arg("image")
            .arg("pull")
            .arg(image.tag.as_image_name())
            .run()?;
        Ok(())
    }
    fn uninstall(&self, distr: &DistributionRef) -> anyhow::Result<()> {
        let image = distr.downcast_ref::<Image>()
            .context("invalid distribution for Docker")?;
        match process::Native::new("image remove", "docker", &self.cli)
            .arg("image")
            .arg("rm")
            .arg(image.tag.as_image_name())
            .run_or_stderr()?
        {
            Ok(_) => {}
            Err((_, text)) if text.contains("No such image") => {},
            Err((s, text)) => anyhow::bail!("docker error: {}: {}", s, text),
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
                    Nightly(slot, _) => {
                        let ver = Version(slot.clone());
                        Some(Image {
                            version_slot: VersionSlot::Nightly(ver.clone()),
                            version: ver,
                            tag: t.clone(),
                        }.into_ref())
                    }
                }
            }).collect())
        } else {
            Ok(self.get_tags()?.iter().filter_map(|t| {
                match t {
                    Stable(v, rev) => Some(Image {
                        version_slot: VersionSlot::Stable(Version(v.clone())),
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
        let data = process::Native::new("image list", "docker", &self.cli)
            .arg("image")
            .arg("list")
            .arg("--no-trunc")
            .arg("--format")
            .arg("{{.Repository}} {{.Tag}} {{.Digest}}")
            .get_stdout_text()?;
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
                process::Native::new("volume remove", "none", &self.cli)
                    .arg("volume")
                    .arg("remove")
                    .arg(name)
                    .run()?;
                Ok(())
            }
            _ => unix::clean_storage(storage),
        }
    }
    fn bootstrap(
        &self, settings: &create_mod::Settings
    ) -> anyhow::Result<()> {
        let volume = match &settings.storage {
            Storage::DockerVolume(name) => name,
            other => anyhow::bail!("unsupported storage {:?}", other),
        };
        let image = settings.distribution.downcast_ref::<Image>()
            .context("invalid unix package")?;
        let user = whoami::username();
        let md = serde_json::to_string(&settings.metadata())?;
        process::Native::new("volume create", "docker", &self.cli)
            .arg("volume")
            .arg("create")
            .arg(volume)
            .arg(format!("--label=com.edgedb.metadata.user={}", user))
            .run()?;

        self.docker_run("chown", image.tag.as_image_name(), "sh")
            .mount(volume, "/mnt")
            .as_root()
            .arg("-c")
            .arg(format!("chown -R 999:999 /mnt"))
            .run()?;

        let cert_required =
            image.version_slot.slot_name() >= &Version("1-beta2");
        let password = generate_password();
        let mut cmd = self.docker_run("server",
            image.tag.as_image_name(), "edgedb-server");
        if cert_required {
            cmd.env("EDGEDB_HIDE_GENERATED_CERT", "1");
        }
        cmd.env_default("EDGEDB_SERVER_LOG_LEVEL", "warn");
        cmd.env_default("EDGEDB_SERVER_DOCKER_LOG_LEVEL", "warning");
        cmd.mount(volume, "/var/lib/edgedb/data");
        cmd.arg("--bootstrap-only");
        cmd.arg("--bootstrap-command")
            .arg(bootstrap_script(settings, &password));
        cmd.arg("--runstate-dir").arg("/var/lib/edgedb/data/run");
        cmd.arg("--data-dir")
           .arg(format!("/var/lib/edgedb/data/{}", settings.name));
        if cert_required {
            cmd.arg("--generate-self-signed-cert");
        }
        cmd.run()?;

        let output = self.docker_run("write metadata",
                image.tag.as_image_name(), "sh")
            .as_root()
            .mount(volume, "/mnt")
            .arg("-c")
            .arg(format!(r###"
                    echo {metadata} > /mnt/{name}/metadata.json
                    {cert_cmd}
                "###,
                name=settings.name,
                metadata=shell_words::quote(&md),
                cert_cmd=if cert_required {
                    format!("cat /mnt/{}/edbtlscert.pem", settings.name)
                } else { "".into() }
            ))
            .get_stdout_text()?;

        let cert = if cert_required {
            let (cstart, cend) = output.find("-----BEGIN CERTIFICATE-----")
                .zip(find_end(&output, "-----END CERTIFICATE-----"))
                .context("Error generating certificate")?;
            Some(&output[cstart..cend])
        } else {
            None
        };

        task::block_on(save_credentials(&settings, &password, cert))?;
        drop(password);

        self.create(&Create {
            name: &settings.name,
            image: &image,
            port: settings.port,
            start_conf: settings.start_conf,
        })?;
        if !settings.suppress_messages {
            println!("To connect, run:\n  edgedb -I {}",
                     settings.name.escape_default());
        }

        Ok(())
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>>
         where Self: 'os
    {
        let output = process::Native::new("volume list", "docker", &self.cli)
            .arg("volume")
            .arg("list")
            .arg("--filter")
            .arg(format!("label=com.edgedb.metadata.user={}",
                          whoami::username()))
            .arg("--format").arg("{{.Name}}")
            .get_stdout_text()?;
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
        -> anyhow::Result<bool>
    {
        use upgrade::ToDo::*;

        match todo {
            MinorUpgrade => {
                self.minor_upgrade(options)
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
        match process::Native::new("volume remove", "docker", &self.cli)
            .arg("volume")
            .arg("remove")
            .arg(&container_name)
            .run_or_stderr()?
        {
            Ok(_) => {
                log::info!(target: "edgedb::server::destroy",
                    "Removed volume {:?}", container_name);
                found = true;
            }
            Err((_, text)) if text.contains("No such volume") => {},
            Err((s, text)) => anyhow::bail!("docker error: {}: {}", s,  text),
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
        let mut cmd = self.method.docker_run("get metadata", "busybox", "cat");
        cmd.as_root(); // TODO(tailhook) is needed?
        cmd.mount(volume_name, "/mnt");
        cmd.arg(format!("/mnt/{}.backup/backup.json", self.name));
        cmd.arg(format!("/mnt/{}.backup/metadata.json", self.name));
        let out = cmd.get_output()?;
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
                .context("can decode error output of docker")?;
            if stderr.contains("No such file or directory") {
                return Ok(BackupStatus::Absent);
            }
            anyhow::bail!(stderr);
        }
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
    fn get_meta(&self) -> anyhow::Result<&Metadata> {
        self.metadata.get_or_try_init(|| {
            let volume_name = self.volume_name();
            let data = self.method.docker_run("get metadata", "busybox", "cat")
                .as_root()  // TODO(tailhook) is needed?
                .mount(volume_name, "/mnt")
                .arg(format!("/mnt/{}/metadata.json", self.name))
                .get_stdout_text()?;
            Ok(serde_json::from_str(&data)?)
        })
    }
    fn get_version(&self) -> anyhow::Result<&VersionMarker> {
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
        let credentials_file_exists = credentials::path(&self.name)
            .map(|path| path.exists())
            .unwrap_or(false);

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
            process::Native::new("container start", "docker", &self.method.cli)
                .arg("container")
                .arg("start")
                .arg("--attach")
                .arg("--interactive")
                .arg(self.container_name())
                .no_proxy().run()?;
        } else {
            process::Native::new("container start", "docker", &self.method.cli)
                .arg("container")
                .arg("start")
                .arg(self.container_name())
                .run()?;
        }
        Ok(())
    }
    fn stop(&self, _options: &Stop) -> anyhow::Result<()> {
        process::Native::new("container stop", "docker", &self.method.cli)
            .arg("container")
            .arg("stop")
            .arg(self.container_name())
            .run()?;
        Ok(())
    }
    fn restart(&self, _options: &Restart) -> anyhow::Result<()> {
        process::Native::new("container restart", "docker", &self.method.cli)
            .arg("container")
            .arg("restart")
            .arg(self.container_name())
            .run()?;
        Ok(())
    }
    fn logs(&self, options: &Logs) -> anyhow::Result<()> {
        let mut cmd = process::Native::new(
            "container logs", "docker", &self.method.cli);
        cmd.arg("container");
        cmd.arg("logs");
        cmd.arg(self.container_name());
        if let Some(n) = options.tail {
            cmd.arg(format!("--tail={}", n));
        }
        if options.follow {
            cmd.arg("--follow");
        }
        cmd.no_proxy().run()
    }
    fn service_status(&self) -> anyhow::Result<()> {
        process::Native::new("container inspect", "docker", &self.method.cli)
            .arg("container")
            .arg("inspect")
            .arg(self.container_name())
            .no_proxy().run()?;
        Ok(())
    }
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        if admin {
            anyhow::bail!("Cannot connect to admin socket in docker")
        } else {
            get_connector(self.name())
        }
    }
    fn get_command(&self) -> anyhow::Result<process::Native> {
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
        self.method.docker_run("clean metadata", "busybox", "sh")
            .mount(volume, "/mnt")
            .arg("-ec")
            .arg(format!(r###"
                    rm -rf /mnt/{name}
                    mv /mnt/{name}.backup /mnt/{name}
                    rm /mnt/{name}/backup.json
                "###,
                name=name,
            ))
            .run()?;

        self.delete()?;
        self.method.create(&create)?;

        let upgrade_container = format!("edgedb_upgrade_{}", name);
        self.method.delete_container(&upgrade_container)?;

        Ok(())
    }
    fn reset_password(&self, user: &str, password: &str) -> anyhow::Result<()>
    {
        let container = self.get_container()?;
        let container = container.as_ref()
            .context("No server container found. Please start the server")?;
        self.method.docker_run("reset cli", container.Image.clone(), "edgedb")
            .mount(self.volume_name(), "/mnt")
            .arg("--admin")
            .arg("--host").arg("/mnt/run")
            .arg("--port").arg(self.get_port()?.to_string())
            .feed(&format!(r###"
                ALTER ROLE {name} {{
                    SET password := {password};
                }};"###,
                name=quote_name(&user),
                password=quote_string(&password))
            )?;
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



fn find_end(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle).map(|x| x + needle.len())
}
