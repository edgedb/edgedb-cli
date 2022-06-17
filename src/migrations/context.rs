use std::path::{Path, PathBuf};

use crate::commands::parser::MigrationConfig;
use crate::portable::config;
use crate::portable::project;


pub struct Context {
    pub schema_dir: PathBuf,
}

impl Context {
    pub fn from_project_or_config(cfg: &MigrationConfig) -> anyhow::Result<Context> {
        let schema_dir = if let Some(schema_dir) = &cfg.schema_dir {
            schema_dir.clone()
        } else if let Some(config_dir_path) = project::search_dir(Path::new(".").as_ref()) {
            let config_path = config_dir_path.join("edgedb.toml");
            let config = config::read(&config_path)?;
            config.project.schema_dir
        } else {
            "./dbschema".into()
        };

        Ok(Context {
            schema_dir,
        })
    }
}
