use std::path::PathBuf;

use serde::Serialize;

use crate::server::detect::{VersionQuery, InstalledPackage, VersionResult};
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::install;
use crate::server::init;
use crate::server::package::PackageInfo;
use crate::server::version::Version;
use crate::server::methods::InstallMethod;


#[derive(Debug, Serialize)]
pub struct DockerCandidate {
    pub supported: bool,
    pub platform_supported: bool,
    cli: Option<PathBuf>,
    socket: Option<PathBuf>,
    socket_permissions_ok: bool,
}

#[derive(Debug, Serialize)]
pub struct DockerMethod<'os, O: CurrentOs + ?Sized> {
    #[serde(skip)]
    os: &'os O,
    cli: PathBuf,
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
        })
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
    fn all_versions(&self, _nightly: bool) -> anyhow::Result<&[PackageInfo]> {
        // TODO(tailhook) implement fetching versions from docker
        Ok(&[])
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
