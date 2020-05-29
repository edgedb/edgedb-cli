use crate::server::methods::{InstallationMethods, InstallMethod};
use crate::server::os_trait::{CurrentOs, Method};

use serde::Serialize;


#[derive(Debug, Serialize)]
pub struct Macos {
}


impl CurrentOs for Macos {
    fn get_available_methods(&self)
        -> Result<InstallationMethods, anyhow::Error>
    {
        todo!();
    }
    fn detect_all(&self) -> serde_json::Value {
        todo!();
    }
    fn make_method<'x>(&'x self, _method: &InstallMethod,
        _methods: &InstallationMethods)
        -> anyhow::Result<Box<dyn Method + 'x>>
    {
        todo!();
    }
}

impl Macos {
    pub fn new() -> Macos {
        Macos {
        }
    }
}
