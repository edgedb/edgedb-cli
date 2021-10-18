use std::collections::BTreeMap;

use once_cell::sync::OnceCell;
use serde::Serialize;

use crate::server::version::Version;
use crate::server::os_trait::CurrentOs;
use crate::server::methods::{self, InstallMethod};

use anyhow::Context;


#[cfg(target_arch="x86_64")]
pub const ARCH: &str = "x86_64";
#[cfg(target_arch="aarch64")]
pub const ARCH: &str = "aarch64";
#[cfg(not(any(
    target_arch="x86_64",
    target_arch="aarch64",
)))]
compile_error!("Unsupported architecture, supported: x86_64, aarch64");

#[derive(Clone, Debug, Default)]
pub struct Lazy<T>(once_cell::sync::OnceCell<T>);

#[derive(Clone, Serialize, Debug)]
pub struct VersionResult {
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub revision: String,
}

#[derive(Clone, Serialize, Debug)]
pub struct InstalledPackage {
    pub package_name: String,
    pub major_version: Version<String>,
    pub version: Version<String>,
    pub revision: String,
}

impl<T: Serialize> Serialize for Lazy<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer
    {
        self.0.get().serialize(serializer)
    }
}

impl<T> Lazy<T> {
    pub fn lazy() -> Lazy<T> {
        Lazy(OnceCell::new())
    }
    pub fn eager(val:T) -> Lazy<T> {
        let cell = OnceCell::new();
        cell.set(val).map_err(|_| "cell failed").unwrap();
        Lazy(cell)
    }
    pub fn get_or_init<F>(&self, f: F) -> &T
        where F: FnOnce() -> T
    {
        self.0.get_or_init(f)
    }
    pub fn get_or_try_init<F, E>(&self, f: F) -> Result<&T, E>
        where F: FnOnce() -> Result<T, E>
    {
        self.0.get_or_try_init(f)
    }
}

pub fn current_os() -> anyhow::Result<Box<dyn CurrentOs>> {
    use crate::server::{windows, macos, linux, unknown_os};

    if cfg!(windows) {
        Ok(Box::new(windows::Windows::new()))
    } else if cfg!(target_os="macos") {
        Ok(Box::new(macos::Macos::new()))
    } else if cfg!(target_os="linux") {
        linux::detect_distro()
            .context("error detecting linux distribution")
    } else {
        Ok(Box::new(unknown_os::Unknown::new()))
    }
}

pub fn main(_arg: &crate::server::options::Detect)
    -> Result<(), anyhow::Error>
{
    #[derive(Serialize)]
    struct Info {
        os_type: &'static str,
        os_info: serde_json::Value,
        detected: methods::InstallationMethods,
        methods: BTreeMap<InstallMethod, serde_json::Value>,
    }

    let os = current_os()?;
    let detected = os.get_available_methods()?;
    let methods = detected.instantiate_all(&*os, true)?;
    serde_json::to_writer_pretty(std::io::stdout(), &Info {
        os_type: os.get_type_name(),
        os_info: os.detect_all(),
        detected,
        methods: methods.iter()
            .map(|(mname, meth)| (mname.clone(), meth.detect_all()))
            .collect(),
    })?;
    Ok(())
}


impl InstalledPackage {
    pub fn is_nightly(&self) -> bool {
        // TODO(tailhook) get nightly flag from the source index
        return self.version.as_ref().contains(".dev")
    }
    pub fn full_version(&self) -> Version<String> {
        Version(format!("{}-{}", self.version, self.revision))
    }
}
