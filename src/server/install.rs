use std::str::FromStr;
use std::process::exit;

use serde::Serialize;
use linked_hash_map::LinkedHashMap;

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
    use InstallMethod::*;
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    if options.method.is_none() && !options.interactive &&
        !avail_methods.package.supported
    {
        anyhow::bail!(avail_methods.format_error());
    }
    let mut methods = LinkedHashMap::new();
    if avail_methods.package.supported {
        methods.insert(Package,
            current_os.make_method(&Package, &avail_methods)?);
    }
    if avail_methods.docker.supported {
        methods.insert(Docker,
            current_os.make_method(&Docker, &avail_methods)?);
    }
    let effective_method = options.method.clone().unwrap_or(Package);
    for (meth_kind, meth) in &methods {
        for old_ver in meth.installed_versions()? {
            if options.version.is_none() ||
                matches!(&options.version,
                         Some(v) if v == &old_ver.major_version)
            {
                if &effective_method == meth_kind {
                    eprintln!("EdgeDB {} ({}) is already installed. \
                        Use `edgedb server upgrade` for upgrade.",
                        old_ver.major_version, old_ver.version);
                } else {
                    eprintln!("EdgeDB {} is already installed via {}. \
                        Please deinstall before installing via {}.",
                        old_ver.major_version, meth_kind.option(),
                        effective_method.option());
                }
                exit(exit_codes::ALREADY_INSTALLED);
            }
        }
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
