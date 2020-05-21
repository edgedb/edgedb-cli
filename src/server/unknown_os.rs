use serde::Serialize;

use crate::server::detect::{InstallationMethods};
use crate::server::install;
use crate::server::os_trait::{CurrentOs, Method};


#[derive(Debug, Serialize)]
pub struct Unknown {
}


impl CurrentOs for Unknown {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        todo!();
    }
    fn detect_all(&self) -> serde_json::Value {
        todo!();
    }
    fn make_method<'x>(&'x self, _method: &install::InstallMethod,
        _methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        todo!();
    }
}

impl Unknown {
    pub fn new() -> Unknown {
        Unknown {
        }
    }
}
