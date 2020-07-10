use std::fs;
use std::str;
use std::path::{Path, PathBuf};
use std::process::{exit, Command as StdCommand};

use anyhow::Context;
use async_std::task;
use serde::Serialize;

use crate::platform::{Uid, get_current_uid, home_dir};
use crate::process::run;
use crate::server::detect::{ARCH, Lazy, VersionQuery, VersionResult};
use crate::server::detect::{InstalledPackage};
use crate::server::docker::DockerCandidate;
use crate::server::init;
use crate::server::install::{self, operation, exit_codes, Operation, Command};
use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::options::StartConf;
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::{PackageMethod, PackageInfo};
use crate::server::package::{self, PackageCandidate, RepositoryInfo};
use crate::server::remote;
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
    fn install(&self, settings: &install::Settings)
        -> Result<(), anyhow::Error>
    {
        let tmpdir = tempfile::tempdir()?;
        let ver = self.get_version(&VersionQuery::new(
            settings.nightly, Some(&settings.major_version)))?;
        let package_name = format!("edgedb-server-{}_{}_{}.pkg",
            settings.major_version, settings.version, ver.revision);
        let pkg_path = tmpdir.path().join(&package_name);
        task::block_on(remote::get_file(
            &pkg_path,
            &format!("https://packages.edgedb.com/archive/macos-{arch}/{name}",
                arch=ARCH, name=package_name)))
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
    fn all_versions(&self, nightly: bool) -> anyhow::Result<&[PackageInfo]> {
        Ok(self.os.get_repo(nightly)?
            .map(|x| &x.packages[..]).unwrap_or(&[]))
    }
    fn get_version(&self, query: &VersionQuery)
        -> anyhow::Result<VersionResult>
    {
        let packages = self.os.get_repo(query.is_nightly())?
            .ok_or_else(|| anyhow::anyhow!("No repository found"))?;
        package::find_version(packages, query)
    }
    fn installed_versions(&self) -> anyhow::Result<&[InstalledPackage]> {
        Ok(&self.installed.get_or_try_init(|| {
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
                let mut pair_iter = version.splitn(2, "_");
                let (ver, rev) = match (pair_iter.next(), pair_iter.next()) {
                    (Some(ver), Some(rev)) => (ver, rev),
                    (Some(ver), None) => (ver, "<unknown>"),
                    _ => unreachable!(),
                };

                result.push(InstalledPackage {
                    package_name: "edgedb-server".to_owned(),
                    major_version: Version(major.to_string()),
                    version: Version(ver.to_owned()),
                    revision: rev.to_owned(),
                });
            }
            Ok(result)
        })?)
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn get_server_path(&self, major_version: &Version<String>)
        -> anyhow::Result<PathBuf>
    {
        get_server_path(major_version)
    }
    fn create_user_service(&self, settings: &init::Settings)
        -> anyhow::Result<()>
    {
        let unit_dir = if settings.system {
            PathBuf::from("/Library/LaunchDaemons")
        } else {
            home_dir()?.join("Library/LaunchAgents")
        };
        fs::create_dir_all(&unit_dir)?;
        let unit_path = unit_dir
            .join(&format!("com.edgedb.edgedb-server-{}.plist",
                           settings.name));
        fs::write(&unit_path, plist_data(&settings)?)?;
        fs::create_dir_all(home_dir()?.join(".edgedb/run"))?;

        run(StdCommand::new("launchctl")
            .arg("load")
            .arg(unit_path))?;
        Ok(())
    }
}

pub fn get_server_path(major_version: &Version<String>)
    -> anyhow::Result<PathBuf>
{
    Ok(Path::new("/Library/Frameworks/EdgeDB.framework/Versions")
        .join(major_version.as_ref())
        .join("lib")
        .join(&format!("edgedb-server-{}", major_version))
        .join("bin/edgedb-server"))
}

fn plist_data(settings: &init::Settings)
    -> anyhow::Result<String>
{
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
        directory=settings.directory.display(),
        server_path=get_server_path(&settings.version)?.display(),
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
