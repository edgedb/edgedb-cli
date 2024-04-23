use edgedb_tokio::get_project_dir;

use crate::commands::Options;
use crate::connect::Connection;
use crate::credentials;
use crate::portable::config::Config;
use crate::portable::options::InstanceName;
use crate::portable::project;
use std::fs;

pub struct Context {
    /// Instance name provided either with --instance or inferred from the project.
    instance_name: Option<String>,

    /// None means that the current branch is unknown because:
    /// - the instance uses the default branch (and we cannot know what
    ///   that is without making a query), or
    /// - we don't know which instance we are connecting to. This might be because:
    ///   - there was neither a project or the --instance option,
    ///   - the project has no linked instance.
    current_branch: Option<String>,
}

impl Context {
    pub async fn new(options: &Options) -> anyhow::Result<Context> {
        // use instance name provided with --instance
        let instance_name = options.conn_params.get()?.instance_name();
        let instance_name = instance_name.map(|n| n.clone().into());
        let mut instance_name = instance_name.map(assume_local_instance).transpose()?;

        // fallback to instance name of the associated project
        if instance_name.is_none() {
            let project_dir = get_project_dir(None, true).await?;

            let name = if let Some(project_dir) = project_dir {
                let stash_dir = project::stash_path(&fs::canonicalize(project_dir)?)?;
                project::instance_name(&stash_dir).ok()
            } else {
                None
            };

            if let Some(name) = name {
                instance_name = Some(assume_local_instance(name)?);
            }
        }

        // try to read current branch from instance credentials
        let current_branch = if let Some(instance_name) = &instance_name {
            let credentials_path = credentials::path(&instance_name)?;
            let credentials = credentials::read(&credentials_path).await?;

            credentials.branch.or(credentials.database)
        } else {
            None
        };

        Ok(Context {
            instance_name,
            current_branch,
        })
    }

    /// Returns the "current" branch. Connection must not have its branch param modified.
    pub async fn get_current_branch(&self, connection: &mut Connection) -> anyhow::Result<String> {
        if let Some(b) = &self.current_branch {
            return Ok(b.clone());
        }

        // if the instance is unknown, current branch is just "the branch of the connection"
        // so we can pull it out here (if it is not the default branch)
        if connection.branch() != "__default__" {
            return Ok(connection.branch().to_string());
        }

        // if the connection branch is the default branch, query the database to see
        // what that default is
        let branch: String = connection
            .query_required_single("select sys::get_current_database()", &())
            .await?;
        Ok(branch)
    }

    pub fn can_update_current_branch(&self) -> bool {
        // we can update the current branch only if we know the instance, so we can write the credentials
        self.instance_name.is_some()
    }

    pub async fn update_current_branch(&self, branch: &str) -> anyhow::Result<()> {
        let Some(instance_name) = &self.instance_name else {
            return Ok(());
        };

        let path = credentials::path(&instance_name)?;
        let mut credentials = credentials::read(&path).await?;
        credentials.database = Some(branch.to_string());
        credentials.branch = Some(branch.to_string());

        credentials::write_async(&path, &credentials).await
    }

    pub async fn get_project_config(&self) -> anyhow::Result<Option<Config>> {
        let project_dir = get_project_dir(None, true).await?;
        let Some(path) = &project_dir else {
            return Ok(None);
        };
        Ok(Some(crate::portable::config::read(
            &path.join("edgedb.toml"),
        )?))
    }
}

fn assume_local_instance(instance_name: InstanceName) -> anyhow::Result<String> {
    match instance_name {
        InstanceName::Local(local) => Ok(local),
        InstanceName::Cloud {
            name: _,
            org_slug: _,
        } => anyhow::bail!("Cannot use instance-name branching on cloud"), // yet
    }
}
