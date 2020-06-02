use serde::Serialize;

use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method};
use crate::server::docker::DockerCandidate;
use crate::server::package::PackageCandidate;


#[derive(Debug, Serialize)]
pub struct Unknown {
}


impl CurrentOs for Unknown {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        Ok(InstallationMethods {
            package: PackageCandidate {
                supported: false,
                distro_name: "<unknown>".into(),
                distro_version: "<unknown>".into(),
                distro_supported: false,
                version_supported: false,
            },
            docker: DockerCandidate::detect()?,
        })
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("can serialize")
    }
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use InstallMethod::*;
        match method {
            Package => anyhow::bail!(
                "Package method is unsupported on current OS"),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

impl Unknown {
    pub fn new() -> Unknown {
        Unknown {
        }
    }
}
