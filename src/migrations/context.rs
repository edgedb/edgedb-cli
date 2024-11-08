use std::path::{Path, PathBuf};

use crate::migrations::options::MigrationConfig;
use crate::portable::config;

use edgedb_tokio::get_project_path;

pub struct Context {
    pub schema_dir: PathBuf,

    pub quiet: bool,
}

impl Context {
    pub async fn from_project_or_config(
        cfg: &MigrationConfig,
        quiet: bool,
    ) -> anyhow::Result<Context> {
        let schema_dir = if let Some(schema_dir) = &cfg.schema_dir {
            schema_dir.clone()
        } else if let Some(config_path) = get_project_path(None, true).await? {
            let config = config::read(&config_path)?;
            config.project.schema_dir
        } else {
            let default_dir: PathBuf = "./dbschema".into();
            if !default_dir.exists() {
                anyhow::bail!("`dbschema` directory doesn't exist. Either create one or provide path via --schema-dir.");
            }
            default_dir
        };

        Ok(Context { schema_dir, quiet })
    }
    pub fn for_watch(config_path: &Path) -> anyhow::Result<Context> {
        let config = config::read(&config_path)?;
        Context::for_project(&config)
    }
    pub fn for_project(config: &config::Config) -> anyhow::Result<Context> {
        Ok(Context {
            schema_dir: config.project.schema_dir.clone(),
            quiet: false,
        })
    }
}
