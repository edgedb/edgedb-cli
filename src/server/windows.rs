use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method};

use serde::Serialize;
use crate::server::docker::DockerCandidate;
use crate::server::package::{PackageCandidate};


#[derive(Debug, Serialize)]
pub struct Windows {
}


impl CurrentOs for Windows {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        Ok(InstallationMethods {
            package: PackageCandidate {
                supported: false,
                distro_name: "Windows".into(),
                distro_version: "".into(), // TODO(tailhook)
                distro_supported: false,
                version_supported: false,
            },
            docker: DockerCandidate::detect()?,
        })
    }
    fn detect_all(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("cannot serialize")
    }
    fn make_method<'x>(&'x self, method: &InstallMethod,
        methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        use InstallMethod::*;

        match method {
            Package => anyhow::bail!("Method `package` is not supported"),
            Docker => Ok(Box::new(methods.docker.make_method(self)?)),
        }
    }
}

impl Windows {
    pub fn new() -> Windows {
        Windows {
        }
    }
}
