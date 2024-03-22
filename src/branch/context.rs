use std::fs;
use crate::portable::config::Config;
use std::path::{Path, PathBuf};
use crate::connect::Connection;
use crate::credentials;
use crate::portable::options::InstanceName;
use crate::portable::project::{instance_name, stash_path};

pub struct Context {
    pub project_config: Option<Config>,
    pub project_branch: Option<String>,
    pub branch: Option<String>,

    project_dir: Option<PathBuf>,
}

impl Context {
    pub async fn get_branch_or_default(&self, connection: &mut Connection) -> anyhow::Result<String> {
        if self.branch.is_some() {
            return Ok(self.branch.as_ref().unwrap().clone());
        }

        return Ok(self.get_default_branch_name(connection).await?.to_string())
    }

    pub async fn new(project_dir: Option<&PathBuf>) -> anyhow::Result<Context> {
        let project_config = match project_dir {
            Some(path) => Some(crate::portable::config::read(&path.join("edgedb.toml"))?),
            None => None
        };

        let project_branch = project_config.as_ref().map(|v| v.edgedb.branch.clone());

        let credentials = match project_dir {
            Some(path) => Some(credentials::read(&credentials::path(&get_instance_name(&path)?)?).await?),
            None => None
        };

        Ok(Context {
            project_config,
            branch: credentials.and_then(|v| v.database).or(project_branch.clone()),
            project_branch,
            project_dir: project_dir.map(PathBuf::from)
        })
    }

    pub async fn get_default_branch_name(&self, connection: &mut Connection) -> anyhow::Result<&str> {
        let version = connection.get_version().await?.specific();

        if version.major >= 5 {
            return Ok("main")
        }

        Ok("edgedb")
    }

    pub fn get_instance_name(&self) -> anyhow::Result<Option<String>> {
        Ok(match &self.project_dir {
            Some(dir) => Some(get_instance_name(&dir)?),
            None => None
        })
    }

    pub async fn update_branch(&self, branch: &String) -> anyhow::Result<()> {
        let instance_name = match self.get_instance_name()? {
            Some(i) => i,
            None => return Ok(())
        };

        let path = credentials::path(&instance_name)?;

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
