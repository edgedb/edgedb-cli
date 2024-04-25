use crate::connect::Connection;
use crate::credentials;
use crate::portable::config::Config;
use crate::portable::options::InstanceName;
use crate::portable::project::{instance_name, stash_path};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Context {
    pub project_config: Option<Config>,
    pub branch: Option<String>,

    project_dir: Option<PathBuf>,
}

impl Context {
    pub async fn new(project_dir: Option<&PathBuf>) -> anyhow::Result<Context> {
        let project_config = match project_dir {
            Some(path) => Some(crate::portable::config::read(&path.join("edgedb.toml"))?),
            None => None,
        };

        let credentials = match project_dir {
            Some(path) => {
                Some(credentials::read(&credentials::path(&get_instance_name(path)?)?).await?)
            }
            None => None,
        };

        Ok(Context {
            project_config,
            branch: credentials.and_then(|v| v.database),
            project_dir: project_dir.map(PathBuf::from),
        })
    }

    pub async fn get_default_branch_name(
        &self,
        connection: &mut Connection,
    ) -> anyhow::Result<&str> {
        let version = connection.get_version().await?.specific();

        if version.major >= 5 {
            return Ok("main");
        }

        Ok("edgedb")
    }

    pub fn get_instance_name(&self) -> anyhow::Result<Option<String>> {
        Ok(match &self.project_dir {
            Some(dir) => Some(get_instance_name(dir)?),
            None => None,
        })
    }

    pub async fn update_branch(&self, branch: &str) -> anyhow::Result<()> {
        let instance_name = match self.get_instance_name()? {
            Some(i) => i,
            None => return Ok(()),
        };

        let path = credentials::path(&instance_name)?;

        let mut credentials = credentials::read(&path).await?;

        credentials.database = Some(branch.to_string());
        credentials.branch = Some(branch.to_string());

        credentials::write_async(&path, &credentials).await
    }
}

fn get_instance_name(project_dir: &Path) -> anyhow::Result<String> {
    match instance_name(&stash_path(&fs::canonicalize(project_dir)?)?)? {
        InstanceName::Local(local) => Ok(local),
        InstanceName::Cloud {
            name: _,
            org_slug: _,
        } => anyhow::bail!("Cannot use instance-name branching on cloud"), // yet
    }
}
