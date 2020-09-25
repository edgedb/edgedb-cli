use std::collections::BTreeSet;
use std::fs;
use std::str;
use std::path::{Path, PathBuf};
use std::process::{exit, Command as StdCommand};

use anyhow::Context;
use async_std::task;
use edgedb_client as client;
use serde::Serialize;
use once_cell::unsync::OnceCell;

use crate::credentials::get_connector;
use crate::platform::{Uid, get_current_uid, home_dir};
use crate::process;
use crate::server::control::read_metadata;
use crate::server::detect::{ARCH, Lazy, VersionQuery};
use crate::server::distribution::{DistributionRef, Distribution, MajorVersion};
use crate::server::docker::DockerCandidate;
use crate::server::errors::InstanceNotFound;
use crate::server::init::{self, Storage};
use crate::server::install::{self, operation, exit_codes, Operation, Command};
use crate::server::metadata::Metadata;
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::options::{StartConf, Start, Stop, Restart, Upgrade};
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
    user_id: Lazy<Uid>,
    sudo_path: Lazy<Option<PathBuf>>,
    #[serde(skip)]
    stable_repo: Lazy<Option<RepositoryInfo>>,
    #[serde(skip)]
    nightly_repo: Lazy<Option<RepositoryInfo>>,
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

impl Macos {
    pub fn get_user_id(&self) -> Uid {
        *self.user_id.get_or_init(|| {
            get_current_uid()
        })
    }
    pub fn get_sudo_path(&self) -> Option<&PathBuf> {
        self.sudo_path.get_or_init(|| {
            which::which("sudo").ok()
        }).as_ref()
    }
}


impl CurrentOs for Macos {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        let version_supported = self.get_repo(false)?
            .map(|repo| repo.packages.iter().any(|p| {
                (p.basename == "edgedb" || p.basename == "edgedb-server")
                && p.architecture == ARCH
            }))
            .unwrap_or(false);
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
    fn detect_all(&self) -> serde_json::Value {
        self.get_user_id();
        self.get_sudo_path();
        serde_json::to_value(self).expect("can serialize")
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
            user_id: Lazy::lazy(),
            sudo_path: Lazy::lazy(),
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

        let mut ctx = operation::Context::new();
        if self.os.get_user_id() != 0 {
            println!("The following commands will be run with elevated \
                privileges using sudo:");
            for op in &operations {
                if op.is_privileged() {
                    println!("    {}", op.format(true));
                }
            }
            println!("Depending on system settings sudo may now ask \
                      you for your password...");
            match self.os.get_sudo_path() {
                Some(cmd) => ctx.set_elevation_cmd(cmd),
                None => {
                    eprintln!("`sudo` command not found. \
                               Cannot elevate acquire needed for \
                               installation. Please run \
                               `edgedb server install` as root user.");
                    exit(exit_codes::NO_SUDO);
                }
            }
        }
        for op in &operations {
            op.perform(&ctx)?;
        }
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
    fn bootstrap(&self, init: &init::Settings) -> anyhow::Result<()> {
        unix::bootstrap(self, init)
    }
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>
    {
        let plist_dir = plist_dir(settings.system)?;
        fs::create_dir_all(&plist_dir)?;
        let plist_path = plist_dir.join(&plist_name(&settings.name));
        fs::write(&plist_path, plist_data(&settings)?)?;
        fs::create_dir_all(home_dir()?.join(".edgedb/run"))?;

        process::run(StdCommand::new("launchctl")
            .arg("load")
            .arg(plist_path))?;
        Ok(())
    }
    fn all_instances<'x>(&'x self) -> anyhow::Result<Vec<InstanceRef<'x>>> {
        let mut instances = BTreeSet::new();
        let user_base = unix::base_data_dir()?;
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
        let dir = unix::base_data_dir()?.join(name);
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
                anyhow::anyhow!("Directory '{}' does not exists", dir.display())
            ).into())
        }
    }
    fn upgrade(&self, todo: &upgrade::ToDo, options: &Upgrade)
        -> anyhow::Result<()>
    {
        unix::upgrade(todo, options, self)
    }
}

impl LocalInstance<'_> {
    fn launchd_name(&self) -> String {
        format!("gui/{}/edgedb-server-{}", get_current_uid(), self.name)
    }
    fn get_meta(&self) -> anyhow::Result<&Metadata> {
        self.metadata.get_or_try_init(|| read_metadata(&self.path))
    }
    fn get_slot(&self) -> anyhow::Result<&String> {
        self.slot.get_or_try_init(|| {
            match &self.get_meta()?.slot {
                Some(s) => Ok(s.clone()),
                None => anyhow::bail!("missing `slot` in metadata"),
            }
        })
    }
    fn unit_path(&self) -> anyhow::Result<PathBuf> {
        let plist = format!("com.edgedb.edgedb-server-{}.plist", &self.name);
        Ok(home_dir()?.join("Library/LaunchAgents").join(plist))
    }
    fn socket_dir(&self) -> anyhow::Result<PathBuf> {
        Ok(home_dir()?
            .join(".edgedb/run")
            .join(&self.name))
    }
}

impl Instance for LocalInstance<'_> {
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
        let service_exists = launchd_plist_path(&self.name, system)
            .map(|p| p.exists())
            .unwrap_or(false);
        unix::status(&self.name, &self.path, service_exists, service)
    }
    fn start(&self, options: &Start) -> anyhow::Result<()> {
        if options.foreground {
            process::run(&mut self.get_command()?)?;
        } else {
            process::run(&mut StdCommand::new("launchctl")
                .arg("load").arg("-w")
                .arg(&self.unit_path()?))?;
        }
        Ok(())
    }
    fn stop(&self, _options: &Stop) -> anyhow::Result<()> {
        process::run(&mut StdCommand::new("launchctl")
            .arg("unload")
            .arg(&self.unit_path()?))?;
        Ok(())
    }
    fn restart(&self, _options: &Restart) -> anyhow::Result<()> {
        process::run(&mut StdCommand::new("launchctl")
            .arg("kickstart")
            .arg("-k")
            .arg(self.launchd_name()))?;
        Ok(())
    }
    fn service_status(&self) -> anyhow::Result<()> {
        process::exit_from(&mut StdCommand::new("launchctl")
            .arg("print")
            .arg(self.launchd_name()))?;
        Ok(())
    }
    fn get_connector(&self, admin: bool) -> anyhow::Result<client::Builder> {
        if admin {
            let socket = self.socket_dir()?
                .join(format!(".s.EDGEDB{}.{}",
                    if admin { ".admin" } else { "" },
                    self.get_meta()?.port));
            let mut conn_params = client::Builder::new();
            conn_params.user("edgedb");
            conn_params.database("edgedb");
            conn_params.unix_addr(socket);
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
        // temporarily patch the edgedb issue of 1-alpha.4
        cmd.arg("--default-database=edgedb");
        cmd.arg("--default-database-user=edgedb");
        Ok(cmd)
    }
}

pub fn get_server_path(slot: &str) -> PathBuf {
    Path::new("/Library/Frameworks/EdgeDB.framework/Versions")
        .join(slot)
        .join("lib")
        .join(&format!("edgedb-server-{}", slot))
        .join("bin/edgedb-server")
}

fn plist_dir(system: bool) -> anyhow::Result<PathBuf> {
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

fn plist_data(settings: &init::Settings) -> anyhow::Result<String> {
    let pkg = settings.distribution.downcast_ref::<Package>()
        .context("invalid macos package")?;
    let path = match &settings.storage {
        Storage::UserDir(path) => path,
        Storage::DockerVolume(..) => {
            anyhow::bail!("launchd units for docker aren't supported");
        }
    };
    Ok(format!(r###"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN"
        "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Disabled</key>
    {disabled}

    <key>Label</key>
    <string>edgedb-server-{instance_name}</string>

    <key>ProgramArguments</key>
    <array>
        <string>{server_path}</string>
        <string>--data-dir={directory}</string>
        <string>--runstate-dir={runtime_dir}</string>
        <string>--port={port}</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    {userinfo}

    <key>KeepAlive</key>
    <dict>
         <key>SuccessfulExit</key>
         <false/>
    </dict>
</dict>
</plist>
"###,
        instance_name=settings.name,
        directory=path.display(),
        server_path=get_server_path(&pkg.slot).display(),
        runtime_dir=home_dir()?
            .join(".edgedb/run").join(&settings.name)
            .display(),
        disabled=match settings.start_conf {
            StartConf::Auto => "<false/>",
            StartConf::Manual => "<true/>",
        },
        port=settings.port,
        userinfo=if settings.system {
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
