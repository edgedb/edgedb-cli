use std::path::PathBuf;

use crate::commands::parser::MigrationConfig;

pub struct Context {
    pub schema_dir: PathBuf,
}

impl Context {
    pub fn from_config(cfg: &MigrationConfig) -> Context {
        Context {
            schema_dir: cfg.schema_dir.clone(),
        }
    }
}
