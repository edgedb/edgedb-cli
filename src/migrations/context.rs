use std::path::{Path, PathBuf};

use crate::commands::parser::MigrationConfig;
use crate::portable::config;


pub struct Context {
    pub schema_dir: PathBuf,
}

impl Context {
    pub fn from_project_or_config(cfg: &MigrationConfig) -> anyhow::Result<Context> {
        let schema_dir = if let Some(schema_dir) = &cfg.schema_dir {
            schema_dir.clone()
        } else if let Some(config_path) = search_project_config_path(&Path::new(".")) {
            let config = config::read(&config_path)?;
            config.edgedb.schema_dir
        } else {
            "./dbschema".into()
        };

        Ok(Context {
            schema_dir,
        })
    }
}


fn search_project_config_path(base_path: &Path) -> Option<PathBuf>
{
    let mut path = base_path;

    let config_path = path.join("edgedb.toml");
    if config_path.exists() {
        return Some(config_path);
    }

    while let Some(parent) = path.parent() {
        let config_path = parent.join("edgedb.toml");
        if config_path.exists() {
            return Some(config_path);
        }
        path = parent;
    }

    None
}
