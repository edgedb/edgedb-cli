use std::path::{Path, PathBuf};

use crate::commands::parser::MigrationConfig;
use crate::portable::config;
use crate::portable::repository::Query;


pub struct Context {
    pub schema_dir: PathBuf,

    /// Version of edgedb declared in edgedb.toml
    ///
    /// It may be set to None just because edgedb.toml has never been read.
    /// (non existing entry in the file is equivalent to "stable").
    pub edgedb_version: Option<Query>,

    pub quiet: bool,
}

#[tokio::main]
async fn get_project_dir(override_dir: Option<&Path>, search_parents: bool)
    -> Result<Option<PathBuf>, edgedb_errors::Error>
{
    edgedb_tokio::get_project_dir(override_dir, search_parents).await
}

impl Context {
    pub fn from_project_or_config(cfg: &MigrationConfig, quiet: bool)
        -> anyhow::Result<Context>
    {
        let mut edgedb_version = None;
        let schema_dir = if let Some(schema_dir) = &cfg.schema_dir {
            schema_dir.clone()
        } else if let Some(cfg_dir) = get_project_dir(None, true)? {
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
        Ok(Context {
            schema_dir: config.project.schema_dir,
            edgedb_version: Some(config.edgedb.server_version),
            quiet: false,
        })
    }
}
