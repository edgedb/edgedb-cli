use std::fs;
use crate::portable::config::Config;
use std::path::{Path, PathBuf};
use crate::credentials;
use crate::portable::options::InstanceName;
use crate::portable::project::{instance_name, stash_path};

pub struct Context {
    pub project_config: Config,
    pub project_branch: String,
    pub branch: String,


    project_dir: PathBuf,
}

impl Context {
    pub async fn new(project_dir: &Path) -> anyhow::Result<Context> {
        let project_config = crate::portable::config::read(&project_dir.join("edgedb.toml"))?;
        let project_branch = project_config.edgedb.branch.clone();

        let path = credentials::path(&get_instance_name(project_dir)?)?;
        let credentials = credentials::read(&path).await?;

        Ok(Context {
            project_config,
            branch: credentials.database.unwrap_or(project_branch.clone()),
            project_branch,
            project_dir: PathBuf::from(project_dir),
        })
    }

    pub fn get_instance_name(&self) -> anyhow::Result<String> {
        get_instance_name(&self.project_dir)
    }

    pub async fn update_branch(&self, branch: &String) -> anyhow::Result<()> {
        let path = credentials::path(&self.get_instance_name()?)?;

        let mut credentials = credentials::read(&path).await?;

        credentials.database = Some(branch.clone());

        credentials::write_async(&path, &credentials).await
    }
}

fn get_instance_name(project_dir: &Path) -> anyhow::Result<String> {
    match instance_name(&stash_path(&fs::canonicalize(project_dir)?)?)? {
        InstanceName::Local(local) => Ok(local),
        InstanceName::Cloud {name: _, org_slug: _} => anyhow::bail!("Cannot use instance-name branching on cloud") // yet
    }
}
