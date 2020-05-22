use std::str::FromStr;

use serde::Serialize;

use crate::server::options::Install;
use crate::server::detect;

pub mod operation;
pub mod exit_codes;
pub mod settings;


pub(in crate::server) use operation::{Operation, Command};
pub(in crate::server) use settings::{Settings, SettingsBuilder};

pub const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";


#[derive(Debug, Clone, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstallMethod {
    Package,
    Docker,
}

pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    let current_os = detect::current_os()?;
    let methods = current_os.get_available_methods()?;
    if options.method.is_none() && !options.interactive &&
        !methods.package.supported
    {
        anyhow::bail!(methods.format_error());
    }
    let mut settings_builder = SettingsBuilder::new(
        &*current_os, options, methods)?;
    settings_builder.auto_version()?;
    // dbg!(&settings_builder);
    let (settings, method) = settings_builder.build()?;
    settings.print();
    method.install(&settings)
}

impl FromStr for InstallMethod {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<InstallMethod> {
        match s {
            "package" => Ok(InstallMethod::Package),
            "docker" => Ok(InstallMethod::Docker),
            _ => anyhow::bail!("Unknown installation method {:?}. \
                Options: package, docker"),
        }
    }
}

impl InstallMethod {
    pub fn title(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "Native System Package",
            Docker => "Docker Container",
        }
    }
    pub fn option(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "--method=package",
            Docker => "--method=docker",
        }
    }
    pub fn short_name(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "package",
            Docker => "docker",
        }
    }
}
