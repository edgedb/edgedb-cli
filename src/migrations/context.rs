use std::path::{Path, PathBuf};

use crate::migrations::options::MigrationConfig;
use crate::portable::config;
use crate::portable::repository::Query;

use edgedb_tokio::get_project_dir;

pub struct Context {
    pub schema_dir: PathBuf,

    /// Version of edgedb declared in edgedb.toml.
    ///
    /// May be set to None if edgedb.toml has never been read, while
    /// a non-existing entry in edgedb.toml will be taken as "stable".
    pub edgedb_version: Option<Query>,

    pub quiet: bool,
}

impl Context {
    pub async fn from_project_or_config(
        cfg: &MigrationConfig,
        quiet: bool,
    ) -> anyhow::Result<Context> {
        let mut edgedb_version = None;
        let schema_dir = if let Some(schema_dir) = &cfg.schema_dir {
            schema_dir.clone()
        } else if let Some(cfg_dir) = get_project_dir(None, true).await? {
            let config_path = cfg_dir.join("edgedb.toml");
            let config = config::read(&config_path)?;
            edgedb_version = Some(config.edgedb.server_version);
            config.project.schema_dir
        } else {
            "./dbschema".into()
        };

        Ok(Context {
            schema_dir,
            edgedb_version,
            quiet,
        })
    }
    pub fn for_watch(project_dir: &Path) -> anyhow::Result<Context> {
        let config_path = project_dir.join("edgedb.toml");
        let config = config::read(&config_path)?;
        Context::for_project(&config)
    }
    pub fn for_project(config: &config::Config) -> anyhow::Result<Context> {
        Ok(Context {
            schema_dir: config.project.schema_dir.clone(),
            edgedb_version: Some(config.edgedb.server_version.clone()),
            quiet: false,
        })
    }
}
