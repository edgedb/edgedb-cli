use std::collections::BTreeSet;
use std::fs;
use std::str;
use std::thread;
use std::time;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand};

use anyhow::Context;
use async_std::task;
use edgedb_client as client;
use fn_error_context::context;
use once_cell::unsync::OnceCell;
use serde::Serialize;

use crate::credentials::{self, get_connector};
use crate::platform::{get_current_uid, home_dir, cache_dir, data_dir};
use crate::process;
use crate::server::control::read_metadata;
use crate::server::create::{self, Storage};
use crate::server::detect::{ARCH, Lazy, VersionQuery};
use crate::server::distribution::{DistributionRef, Distribution, MajorVersion};
use crate::server::docker::DockerCandidate;
use crate::server::errors::InstanceNotFound;
use crate::server::install::{self, Operation, Command};
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::options::{Start, Stop, Restart, Upgrade, Destroy, Logs};
use crate::server::options::{StartConf};
use crate::server::os_trait::{CurrentOs, Method, Instance, InstanceRef};
use crate::server::package::{PackageMethod, Package};
use crate::server::package::{self, PackageCandidate, RepositoryInfo};
use crate::server::remote;
use crate::server::status::{Service, Status};
use crate::server::unix;
use crate::server::upgrade;
use crate::server::version::Version;


#[derive(Debug, Serialize)]
pub struct Macos {
    #[serde(flatten)]
    unix: unix::Unix,
    #[serde(skip)]
    stable_repo: Lazy<Option<RepositoryInfo>>,
    #[serde(skip)]
    nightly_repo: Lazy<Option<RepositoryInfo>>,
}

fn package_name(pkg: &Package) -> String {
    format!("com.edgedb.edgedb-server-{}", pkg.slot)
}

pub struct StatusCache {
    launchctl_list: OnceCell<anyhow::Result<String>>,
}

#[derive(Debug)]
pub struct LocalInstance<'a> {
    pub name: String,
    pub path: PathBuf,
    metadata: Lazy<Metadata>,
    slot: Lazy<String>,
    method: &'a PackageMethod<'a, Macos>,
    current_version: Lazy<Version<String>>,
}


impl CurrentOs for Macos {
    fn refresh_available_methods(&self) -> anyhow::Result<InstallationMethods>
    {
        self.available_methods(true)
    }
    fn get_available_methods(&self) -> anyhow::Result<InstallationMethods> {
        self.available_methods(false)
    }
    fn detect_all(&self) -> serde_json::Value {
        self.unix.detect_all();
        serde_json::to_value(self).expect("cannot serialize")
    }
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use InstallMethod::*;

        match method {
            Package => Ok(Box::new(methods.package.make_method(self)?)),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

impl Macos {
    pub fn new() -> Macos {
        Macos {
            unix: unix::Unix::new(),
            stable_repo: Lazy::lazy(),
            nightly_repo: Lazy::lazy(),
        }
    }
}

impl Macos {
    fn get_repo(&self, nightly: bool)
        -> anyhow::Result<Option<&RepositoryInfo>>
    {
        if nightly {
            self.nightly_repo.get_or_try_init(|| {
                task::block_on(remote::get_json_opt(
                    &format!("https://packages.edgedb.com/archive/\
                        .jsonindexes/macos-{}.nightly.json", ARCH),
                    "failed to fetch repository index"))
            }).map(|opt| opt.as_ref())
        } else {
            self.stable_repo.get_or_try_init(|| {
                Ok(task::block_on(remote::get_json_opt(
                    &format!("https://packages.edgedb.com/archive/\
                        .jsonindexes/macos-{}.json", ARCH),
                    "failed to fetch repository index"))?
                .map(|mut repo: RepositoryInfo| {
                    repo.packages
                        .retain(|p| p.basename == "edgedb-server" &&
                                    // TODO(tailhook) remove this check when
                                    // jsonindexes is fixed
                                    !p.revision.contains("nightly")
                        );
                    repo
                }))
            }).map(|opt| opt.as_ref())
        }
    }
    fn available_methods(&self, refresh: bool)
        -> Result<InstallationMethods, anyhow::Error>
    {
        let version_supported = unix::cache_package_support(
            "MacOS", "", ARCH, refresh,
            || {
                let found = self.get_repo(false)?
                    .map(|repo| repo.packages.iter().any(|p| {
                        (p.basename == "edgedb" ||
                         p.basename == "edgedb-server")
                        && p.architecture == ARCH
                    }))
                    .unwrap_or(false);
                Ok(found)
            },
        )?;
        Ok(InstallationMethods {
            package: PackageCandidate {
                supported: version_supported,
                distro_name: "MacOS".into(),
                distro_version: "".into(), // TODO(tailhook)
                distro_supported: true,
                version_supported,
            },
            docker: DockerCandidate::detect()?,
        })
    }
}

impl<'os> Method for PackageMethod<'os, Macos> {
    fn name(&self) -> InstallMethod {
        InstallMethod::Package
    }
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        let pkg = settings.distribution.downcast_ref::<Package>()
            .context("invalid macos package")?;
        let tmpdir = tempfile::tempdir()?;
        let package_name = format!("edgedb-server-{}_{}.pkg",
            pkg.slot, pkg.version.as_ref().replace("-", "_"));
        let pkg_path = tmpdir.path().join(&package_name);
        let url = if settings.distribution.major_version().is_nightly() {
            format!("https://packages.edgedb.com/archive/\
                macos-{arch}.nightly/{name}",
                arch=ARCH, name=package_name)
        } else {
            format!("https://packages.edgedb.com/archive/\
                macos-{arch}/{name}",
                arch=ARCH, name=package_name)
        };
        task::block_on(remote::get_file(&pkg_path, &url))
            .context("failed to download package")?;

        let operations = vec![
            Operation::PrivilegedCmd(
                Command::new("installer")
                .arg("-package").arg(pkg_path)
                .arg("-target").arg("/")
                .env("_EDGEDB_INSTALL_SKIP_BOOTSTRAP", "1")
            )
        ];

        self.os.unix.perform(operations,
            "installation",
            "edgedb server install")?;
        Ok(())
    }
    fn uninstall(&self, distr: &DistributionRef)
        -> Result<(), anyhow::Error>
    {
        let pkg = distr.downcast_ref::<Package>()
            .context("invalid macos package")?;
        let entries = get_package_paths(&pkg)?;
        let operations = vec![
            Operation::PrivilegedCmd(Command::new("rm")
                .arg("-rf")
                .args(entries)),
            Operation::PrivilegedCmd(Command::new("pkgutil")
                .arg("--forget")
                .arg(package_name(&pkg))),
        ];
        self.os.unix.perform(operations,
            "uninstallation",
            "edgedb server uninstall")?;
        Ok(())
    }
    fn all_versions(&self, nightly: bool)
        -> anyhow::Result<Vec<DistributionRef>>
    {
        Ok(self.os.get_repo(nightly)?
            .map(|x| {
                x.packages.iter()
                .filter(|p| p.basename == "edgedb-server" && p.slot.is_some())
                .map(|p| p.into())
                .collect()
            }).unwrap_or_else(Vec::new))
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<DistributionRef>
    {
        let packages = self.os.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        package::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<Vec<DistributionRef>> {
        Ok(self.installed.get_or_try_init(|| {
            let mut cmd = StdCommand::new("pkgutil");
            cmd.arg(r"--pkgs=com.edgedb.edgedb-server-\d.*");
            let out = cmd.output()
                .context("cannot get installed packages")?;
            if out.status.code() == Some(1) {
                return Ok(Vec::new());
            } else if !out.status.success() {
                anyhow::bail!("cannot get installed packages: {:?} {}",
                    cmd, out.status);
            }
            let mut result = Vec::new();
            let lines = out.stdout.split(|&b| b == b'\n')
                .filter_map(|line| str::from_utf8(line).ok());
            for line in lines {
                if !line.starts_with("com.edgedb.edgedb-server-") {
                    continue;
                }
                let major = &line["com.edgedb.edgedb-server-".len()..].trim();

                let mut cmd = StdCommand::new("pkgutil");
                cmd.arg("--pkg-info").arg(line.trim());
                let out = cmd.output()
                    .context("cannot get package version")?;
                if !out.status.success() {
                    anyhow::bail!("cannot get package version: {:?} {}",
                        cmd, out.status);
                }
                let lines = out.stdout.split(|&b| b == b'\n')
                    .filter_map(|line| str::from_utf8(line).ok());
                let mut version = None;
                for line in lines {
                    if line.starts_with("version:") {
                        version = Some(line["version:".len()..].trim());
                        break;
                    }
                }
                let version = if let Some(version) = version {
                    version
                } else {
                    log::info!("Cannot get version of {:?}", line);
                    continue;
                };

                result.push(Package {
                    major_version: if version.contains(".dev") {
                        MajorVersion::Nightly
                    } else {
                        MajorVersion::Stable(Version(major.to_string()))
                    },
                    version: Version(version.to_string()),
                    slot: major.to_string(),
                }.into_ref());
            }
            Ok(result)
        })?.clone())
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn get_storage(&self, system: bool, name: &str)-> anyhow::Result<Storage> {
        unix::storage(system, name)
    }
    fn storage_exists(&self, storage: &Storage) -> anyhow::Result<bool> {
        unix::storage_exists(storage)
    }
    fn clean_storage(&self, storage: &Storage) -> anyhow::Result<()> {
        unix::clean_storage(storage)
    }
    fn bootstrap(&self, init: &create::Settings) -> anyhow::Result<()> {
        unix::bootstrap(self, init)
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>> {
        let mut instances = BTreeSet::new();
        let user_base = data_dir()?;
        if user_base.exists() {
            unix::instances_from_data_dir(&user_base, false, &mut instances)?;
        }
        Ok(instances.into_iter()
            .map(|(name, _)| LocalInstance {
                method: self,
                path: user_base.join(&name),
                name,
                metadata: Lazy::lazy(),
                slot: Lazy::lazy(),
                current_version: Lazy::lazy(),
            }.into_ref())
            .collect())
    }
    fn get_instance<'x>(&'x self, name: &str)
        -> anyhow::Result<InstanceRef<'x>>
    {
        let dir = unix::storage_dir(name)?;
        if dir.exists() {
            Ok(LocalInstance {
                method: self,
                path: dir,
                name: name.to_owned(),
                metadata: Lazy::lazy(),
                slot: Lazy::lazy(),
                current_version: Lazy::lazy(),
            }.into_ref())
        } else {
            Err(InstanceNotFound(
                anyhow::anyhow!("Directory '{}' does not exist", dir.display())
            ).into())
        }
    }
    fn upgrade(&self, todo: &upgrade::ToDo, options: &Upgrade)
        -> anyhow::Result<bool>
    {
        unix::upgrade(todo, options, self)
    }
    fn destroy(&self, options: &Destroy) -> anyhow::Result<()> {
        if is_service_loaded(&options.name) {
            // bootout will fail if the service is not loaded (e.g. manually-
            // starting services that never started after reboot), also it's
            // unnecessary to unload the service if it wasn't loaded.
            log::info!(target: "edgedb::server::destroy",
                       "Unloading service");
            bootout_launchctl_service(&options.name)?;
        }

        let mut found = false;
        let system = false;
        let unit_path = launchd_plist_path(&options.name, system)?;
        if unit_path.exists() {
            found = true;
            log::info!(target: "edgedb::server::destroy",
                "Removing unit file {}", unit_path.display());
            fs::remove_file(unit_path)?;
        }
        let dir = unix::storage_dir(&options.name)?;
        if dir.exists() {
            found = true;
            log::info!(target: "edgedb::server::destroy",
                "Removing data directory {}", dir.display());
            fs::remove_dir_all(&dir)?;
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

impl LocalInstance<'_> {
    fn launchd_name(&self) -> String {
        launchd_name(&self.name)
    }
    fn get_slot(&self) -> anyhow::Result<&String> {
        self.slot.get_or_try_init(|| {
            match &self.get_meta()?.slot {
                Some(s) => Ok(s.clone()),
                None => anyhow::bail!("missing `slot` in metadata"),
            }
        })
    }
    fn socket_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(runtime_dir(&self.name)?)
    }
}

impl<'a> Instance for LocalInstance<'a> {
    fn get_meta(&self) -> anyhow::Result<&Metadata> {
        self.metadata.get_or_try_init(|| read_metadata(&self.path))
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn method(&self) -> &dyn Method {
        self.method
    }
    fn get_version(&self) -> anyhow::Result<&MajorVersion> {
        Ok(&self.get_meta()?.version)
    }
    fn get_current_version(&self) -> anyhow::Result<Option<&Version<String>>> {
        let meta = self.get_meta()?;
        if meta.version.is_nightly() {
            Ok(self.get_meta()?.current_version.as_ref())
        } else {
            self.current_version.get_or_try_init(|| {
                Ok(self.method.get_version(&meta.version.to_query())?
                    .version().clone())
            }).map(Some)
        }
    }
    fn get_port(&self) -> anyhow::Result<u16> {
        Ok(self.get_meta()?.port)
    }
    fn get_start_conf(&self) -> anyhow::Result<StartConf> {
        Ok(self.get_meta()?.start_conf)
    }
    fn get_status(&self) -> Status {
        let system = false;
        let service = launchctl_status(&self.name, system,
            // TODO
            &StatusCache::new());
        let service_exists = match self.get_start_conf() {
            Ok(StartConf::Auto) => launchd_plist_path(&self.name, system)
                .map(|p| p.exists())
                .unwrap_or(false),
            _ => false,
        };
        unix::status(&self.name, &self.path, service_exists, service)
    }
    fn start(&self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            process::run(&mut self.get_command()?)?;
        } else if self.get_start_conf()? == StartConf::Auto ||
                is_service_loaded(&self.name) {
            // For auto-starting services, we assume they are already loaded.
            // If the server is already running, kickstart won't do anything;
            // or else it will try to (re-)start the server.
            let lname = self.launchd_name();
            process::run(
                StdCommand::new("launchctl").arg("kickstart").arg(&lname)
            )?;
        } else {
            bootstrap_launchctl_service(&self.name, self.get_meta()?)?;
        }
        Ok(())
    }
    fn stop(&self, _options: &Stop) -> anyhow::Result<()> {
        let mut signal_sent = false;
        let deadline = time::Instant::now() + time::Duration::from_secs(30);
        let lname = self.launchd_name();
        loop {
            match launchctl_status(&self.name, false, &StatusCache::new()) {
                Service::Running {..} => {
                    if signal_sent {
                        if time::Instant::now() > deadline {
                            log::warn!(target: "edgedb::server::stop",
                                       "Timing out; send SIGKILL now.");
                            process::run(StdCommand::new("launchctl")
                                .arg("kill")
                                .arg("SIGKILL")
                                .arg(&lname)
                            )?;
                            break;
                        }
                        thread::sleep(time::Duration::from_secs_f32(0.3));
                    } else {
                        process::run(StdCommand::new("launchctl")
                            .arg("kill")
                            .arg("SIGTERM")
                            .arg(&lname)
                        )?;
                        signal_sent = true;
                    }
                },
                Service::Failed {..} => {
                    // Successfully stopped
                    break;
                },
                Service::Inactive { error } => {
                    log::info!(target: "edgedb::server::stop", "{}", error);
                    break;
                }
            }
        }
        Ok(())
    }
    fn restart(&self, _options: &Restart) -> anyhow::Result<()> {
        if self.get_start_conf()? == StartConf::Auto ||
                is_service_loaded(&self.name) {
            // Only use kickstart -k to restart the service if it's loaded
            // already, or it will fail with an error. We assume the service is
            // loaded for auto-starting services.
            process::run(&mut StdCommand::new("launchctl")
                .arg("kickstart")
                .arg("-k")
                .arg(self.launchd_name())
            )?;
        } else {
            bootstrap_launchctl_service(&self.name, self.get_meta()?)?;
        }
        Ok(())
    }
    fn service_status(&self) -> anyhow::Result<()> {
        if is_service_loaded(&self.name) {
            process::exit_from(&mut StdCommand::new("launchctl")
                .arg("print")
                .arg(self.launchd_name()))?;
        } else {
            // launchctl print will fail if the service is not loaded, let's
            // just give a more understandable error here.
            log::error!(target: "edgedb::server::status",
                        "Service is not loaded");
        }
        Ok(())
    }
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        if admin {
            let socket = self.socket_dir()?
                .join(format!(".s.EDGEDB{}.{}",
                    if admin { ".admin" } else { "" },
                    self.get_meta()?.port));
            let mut conn_params = client::Builder::uninitialized();
            conn_params.user("edgedb");
            conn_params.database("edgedb");
            conn_params.host(socket.to_str().context("bad runtime dir")?);
            Ok(conn_params)
        } else {
            get_connector(self.name())
        }
    }
    fn get_command(&self) -> anyhow::Result<StdCommand> {
        let socket_dir = self.socket_dir()?;
        let mut cmd = StdCommand::new(get_server_path(&self.get_slot()?));
        cmd.arg("--port").arg(self.get_meta()?.port.to_string());
        cmd.arg("--data-dir").arg(&self.path);
        cmd.arg("--runstate-dir").arg(&socket_dir);
        cmd.env("EDGEDB_SERVER_INSTANCE_NAME", self.name());
        cmd.env("EDGEDB_SERVER_ALLOW_INSECURE_HTTP_CLIENTS", "1");
        Ok(cmd)
    }
    fn upgrade(&self, meta: &Metadata)
        -> anyhow::Result<InstanceRef<'_>>
    {
        Ok(LocalInstance {
            method: self.method,
            name: self.name.clone(),
            path: self.path.clone(),
            slot: Lazy::eager(meta.slot.as_ref()
                .expect("macos packages always have a slot").clone()),
            current_version: Lazy::eager(meta.current_version.as_ref()
                .expect("current version is known during upgrade").clone()),
            metadata: Lazy::eager(meta.clone()),
        }.into_ref())
    }
    fn revert(&self, metadata: &Metadata)
        -> anyhow::Result<()>
    {
        unix::revert(self, metadata)
    }
    fn logs(&self, options: &Logs) -> anyhow::Result<()> {
        let mut cmd = StdCommand::new("tail");
        if let Some(n) = options.tail {
            cmd.arg("-n").arg(n.to_string());
        }
        if options.follow {
            cmd.arg("-F");
        }
        cmd.arg(log_file(&self.name)?);
        process::run(&mut cmd)
    }
}

pub fn get_server_path(slot: &str) -> PathBuf {
    Path::new("/Library/Frameworks/EdgeDB.framework/Versions")
        .join(slot)
        .join("lib")
        .join(&format!("edgedb-server-{}", slot))
        .join("bin/edgedb-server")
}

pub fn plist_dir(system: bool) -> anyhow::Result<PathBuf> {
    if system {
        Ok(PathBuf::from("/Library/LaunchDaemons"))
    } else {
        Ok(home_dir()?.join("Library/LaunchAgents"))
    }
}

fn plist_name(name: &str) -> String {
    format!("com.edgedb.edgedb-server-{}.plist", name)
}

pub fn launchd_plist_path(name: &str, system: bool)
    -> anyhow::Result<PathBuf>
{
    Ok(plist_dir(system)?.join(plist_name(name)))
}

fn plist_data(name: &str, meta: &Metadata)
    -> anyhow::Result<String>
{
    let system = false;
    let path = get_server_path(
        meta.slot.as_ref().ok_or_else(|| anyhow::anyhow!("no slot on MacOS"))?
    );
    Ok(format!(r###"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN"
        "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>edgedb-server-{instance_name}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{server_path}</string>
        <string>--data-dir={directory}</string>
        <string>--runstate-dir={runtime_dir}</string>
        <string>--port={port}</string>
    </array>

    <key>EnvironmentVariables</key>
    <dict>
        <key>EDGEDB_SERVER_INSTANCE_NAME</key>
        <string>{instance_name}</string>
        <key>EDGEDB_SERVER_ALLOW_INSECURE_HTTP_CLIENTS</key>
        <string>1</string>
    </dict>

    <key>StandardOutPath</key>
    <string>{log_path}</string>
    <key>StandardErrorPath</key>
    <string>{log_path}</string>

    {userinfo}

    <key>KeepAlive</key>
    <dict>
         <key>SuccessfulExit</key>
         <false/>
    </dict>
</dict>
</plist>
"###,
        instance_name=name,
        directory=unix::storage_dir(name)?.display(),
        server_path=path.display(),
        runtime_dir=runtime_dir(&name)?.display(),
        log_path=log_file(&name)?.display(),
        port=meta.port,
        userinfo=if system {
            "<key>UserName</key><string>edgedb</string>"
        } else {
            ""
        },
    ))
}

impl StatusCache {
    pub fn new() -> StatusCache {
        StatusCache {
            launchctl_list: OnceCell::new(),
        }
    }
}

pub fn launchctl_status(name: &str, _system: bool, cache: &StatusCache)
    -> Service
{
    use Service::*;
    let list = cache.launchctl_list.get_or_init(|| {
        process::get_text(&mut StdCommand::new("launchctl").arg("list"))
    });
    let txt = match list {
        Ok(txt) => txt,
        Err(e) => {
            return Service::Inactive {
                error: format!("cannot determine service status: {:#}", e),
            }
        }
    };
    let svc_name = format!("edgedb-server-{}", name);
    for line in txt.lines() {
        let mut iter = line.split_whitespace();
        let pid = iter.next().unwrap_or("-");
        let exit_code = iter.next();
        let cur_name = iter.next();
        if let Some(cur_name) = cur_name {
            if cur_name == svc_name {
                if pid == "-" {
                    return Failed {
                        exit_code: exit_code.and_then(|v| v.parse().ok()),
                    };
                }
                match pid.parse() {
                    Ok(pid) => return Running { pid },
                    Err(e) => return Inactive {
                        error: format!("invalid pid {:?}: {}", pid, e),
                    },
                }
            }
        }
    }
    Inactive { error: format!("service {:?} not found", svc_name) }
}

pub fn is_service_loaded(name: &str) -> bool {
    match launchctl_status(name, false, &StatusCache::new()) {
        Service::Inactive {..} => false,
        _ => true,
    }
}

fn runtime_base() -> anyhow::Result<PathBuf> {
    Ok(cache_dir()?.join("run"))
}

fn runtime_dir(name: &str) -> anyhow::Result<PathBuf> {
    Ok(runtime_base()?.join(name))
}

fn log_file(name: &str) -> anyhow::Result<PathBuf> {
    Ok(cache_dir()?.join(format!("logs/{}.log", name)))
}

fn bootout_launchctl_service(name: &str) -> anyhow::Result<()> {
    let unit_name = launchd_name(name);
    process::run(
        StdCommand::new("launchctl").arg("bootout").arg(&unit_name),
    )?;
    Ok(())
}

fn bootstrap_launchctl_service(name: &str, meta: &Metadata)
    -> anyhow::Result<()>
{
    let plist_dir_path;
    let tmpdir;
    if meta.start_conf == StartConf::Auto {
        plist_dir_path = plist_dir(false)?;
        fs::create_dir_all(&plist_dir_path)?;
    } else {
        tmpdir = tempfile::tempdir()?;
        plist_dir_path = tmpdir.path().to_path_buf();
    }
    let plist_path = plist_dir_path.join(&plist_name(name));
    let unit_name = launchd_name(name);
    fs::write(&plist_path, plist_data(name, meta)?)?;
    fs::create_dir_all(runtime_base()?)?;

    // Clear the disabled status of the unit name, in case the user disabled
    // a service with the same name some time ago and it's likely forgotten
    // because the user is now creating a new service with the same name.
    // This doesn't make the service auto-starting, because we're "hiding" the
    // plist file from launchd if the service is configured as manual start.
    // Actually it is necessary to clear the disabled status even for manually-
    // starting services, because manual start won't work on disabled services.
    process::run(
        StdCommand::new("launchctl").arg("enable").arg(&unit_name),
    )?;
    process::run(StdCommand::new("launchctl")
        .arg("bootstrap")
        .arg(get_domain_target())
        .arg(plist_path)
    )?;

    Ok(())
}

pub fn create_launchctl_service(name: &str, meta: &Metadata)
    -> anyhow::Result<()>
{
    if is_service_loaded(name) {
        bootout_launchctl_service(name)?;
    }
    if meta.start_conf == StartConf::Auto {
        bootstrap_launchctl_service(name, meta)
    } else {
        Ok(())
    }
}

pub fn recreate_launchctl_service(instance: InstanceRef)
    -> anyhow::Result<()>
{
    let name = instance.name();
    let is_running = match launchctl_status(name, false, &StatusCache::new()) {
        Service::Running {..} => {
            log::info!("Stopping instance {:?}", name);
            instance.stop(&Stop { name: name.into() })?;
            bootout_launchctl_service(name)?;
            true
        },
        Service::Failed {..} => {
            bootout_launchctl_service(name)?;
            false
        },
        _ => false,
    };

    let meta = instance.get_meta()?;
    if is_running || meta.start_conf == StartConf::Auto {
        log::info!("Updating service file for instance {:?}", name);
        bootstrap_launchctl_service(name, meta)?;
        log::info!("Started instance {:?}", name);
    }
    Ok(())
}

fn get_domain_target() -> String {
    format!("gui/{}", get_current_uid())
}

fn launchd_name(name: &str) -> String {
    format!("{}/edgedb-server-{}", get_domain_target(), name)
}

#[context("cannot scan package paths of edgedb-server-{}", pkg.slot)]
fn get_package_paths(pkg: &Package) -> anyhow::Result<Vec<PathBuf>> {
    let root = PathBuf::from("/");
    let paths: BTreeSet<_> = process::get_text(
        &mut StdCommand::new("pkgutil")
            .arg("--files")
            .arg(package_name(pkg))
        )?
        .lines()
        .map(|p| root.join(p))
        .collect();
    let mut exclude1 = BTreeSet::new();
    for path in &paths {
        if path.is_dir() {
            let mut dir = fs::read_dir(path)?;
            while let Some(entry) = dir.next().transpose()? {
                if !paths.contains(&path.join(entry.file_name())) {
                    exclude1.insert(path.clone());
                    break;
                }
            }
        }
    }
    let mut exclude2 = BTreeSet::new();
    for path in &exclude1 {
        for parent in path.ancestors() {
            exclude2.insert(parent);
        }
    }
    let mut result = Vec::new();
    for path in paths {
        if exclude1.contains(&path) || exclude2.contains(path.as_path()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            if exclude1.contains(parent) {
                result.push(path);
            }
        }
    }
    Ok(result)
}
