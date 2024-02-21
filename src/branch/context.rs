use std::path::{Path, PathBuf};
use crate::branch::config;
use crate::branch::config::Config as AutoConfig;
use crate::portable::config::Config as ProjectConfig;

pub struct Context {
    pub project_config: ProjectConfig,
    pub auto_config: AutoConfig,

    project_dir: PathBuf
}

impl Context {
    pub fn new(project_dir: &Path) -> anyhow::Result<Context> {
        let project_config = crate::portable::config::read(&project_dir.join("edgedb.toml"))?;
        let branch = project_config.edgedb.branch.clone();
        Ok(Context {
            project_config,
            auto_config: config::create_or_read(&project_dir.join("edgedb.auto.toml"), Some(&branch))?,
            project_dir: PathBuf::from(project_dir)
        })
    }

    pub fn update_branch(&self, branch: &String) -> anyhow::Result<bool> {
        config::modify_current_branch(&self.project_dir.join("edgedb.auto.toml"), branch)
    }
}