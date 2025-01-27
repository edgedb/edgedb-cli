use std::path::PathBuf;

use crate::migrations::options::MigrationConfig;
use crate::portable::project;

use gel_tokio::get_project_path;

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
        } else if let Some(manifest_path) = get_project_path(None, true).await? {
            let config = project::manifest::read(&manifest_path)?;
            config
                .project()
                .resolve_schema_dir(manifest_path.parent().unwrap())?
        } else {
            let default_dir: PathBuf = "./dbschema".into();
            if !default_dir.exists() {
                anyhow::bail!("`dbschema` directory doesn't exist. Either create one or provide path via --schema-dir.");
            }
            default_dir
        };

        Ok(Context { schema_dir, quiet })
    }
    pub fn for_project(project: &project::Context) -> anyhow::Result<Context> {
        Ok(Context {
            schema_dir: project
                .manifest
                .project()
                .resolve_schema_dir(&project.location.root)?,
            quiet: false,
        })
    }
}
