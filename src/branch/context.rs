use edgedb_tokio::{get_project_path, get_stash_path};

use crate::commands::Options;
use crate::connect::Connection;
use crate::credentials;
use crate::platform::tmp_file_path;
use crate::portable::config::Config;
use crate::portable::options::InstanceName;
use crate::portable::project;
use std::fs;
use std::path::PathBuf;

pub struct Context {
    /// Instance name provided either with --instance or inferred from the project.
    instance_name: Option<InstanceName>,

    /// None means that the current branch is unknown because:
    /// - the instance uses the default branch (and we cannot know what
    ///   that is without making a query), or
    /// - we don't know which instance we are connecting to. This might be because:
    ///   - there was neither a project or the --instance option,
    ///   - the project has no linked instance.
    current_branch: Option<String>,

    /// Project dir if it was resolved by instance_name or current directory
    project_dir: Option<PathBuf>,
}

impl Context {
    pub async fn new(options: &Options) -> anyhow::Result<Context> {
        // use instance name provided with --instance
        let instance_name = options.conn_params.get()?.instance_name();
        let mut instance_name: Option<InstanceName> = instance_name.map(|n| n.clone().into());
        let project_file = get_project_path(None, true).await?;
        let project_dir = project_file.as_ref().map(|p| p.parent().unwrap());
        let mut branch: Option<String> = None;

        if instance_name.is_none() {
            instance_name = if let Some(project_dir) = project_dir.as_ref() {
                let stash_dir = get_stash_path(project_dir)?;
                project::instance_name(&stash_dir).ok()
            } else {
                None
            };
        }

        // read from credentials
        if project_dir.is_some()
            && instance_name
                .as_ref()
                .map_or(false, |v| matches!(v, InstanceName::Local(_)))
        {
            let instance_name = match instance_name.as_ref().unwrap() {
                InstanceName::Local(instance) => instance,
                InstanceName::Cloud { org_slug, name } => anyhow::bail!(
                    // should never occur because of the above check
                    format!(
                        "cannot use Cloud instance {}/{}: instance is not linked to a project",
                        org_slug, name
                    )
                ),
            };

            let credentials_path = credentials::path(instance_name)?;
            if credentials_path.exists() {
                let credentials = credentials::read(&credentials_path).await?;
                branch = credentials.branch.or(credentials.database);
            }
        } else if let Some(project_dir) = project_dir.as_ref() {
            // try read from the database file
            let stash_dir = get_stash_path(project_dir)?;
            branch = project::database_name(&stash_dir)?;
        }

        Ok(Context {
            project_dir: project_dir.map(|p| p.to_owned()),
            instance_name,
            current_branch: branch,
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
        Ok(connection.get_current_branch().await?.to_string())
    }

    pub fn can_update_current_branch(&self) -> bool {
        // we can update the current branch only if we know the instance, so we can write the credentials
        self.instance_name.is_some()
    }

    pub async fn update_current_branch(&self, branch: &str) -> anyhow::Result<()> {
        let Some(instance_name) = &self.instance_name else {
            return Ok(());
        };

        match instance_name {
            InstanceName::Local(local_instance_name) => {
                let path = credentials::path(local_instance_name)?;
                let mut credentials = credentials::read(&path).await?;
                credentials.database = Some(branch.to_string());
                credentials.branch = Some(branch.to_string());

                credentials::write_async(&path, &credentials).await?;

                Ok(())
            }
            InstanceName::Cloud {
                org_slug: _org_slug,
                name: _name,
            } if self.project_dir.is_some() => {
                // only place to store the branch is the database file in the project
                let stash_path =
                    get_stash_path(self.project_dir.as_ref().unwrap())?.join("database");

                // ensure that the temp file is created in the same directory as the 'database' file
                let tmp = tmp_file_path(&stash_path);
                fs::write(&tmp, branch)?;
                fs::rename(&tmp, &stash_path)?;

                Ok(())
            }
            InstanceName::Cloud {
                org_slug: org,
                name: inst,
            } => {
                anyhow::bail!(
                    format!("cannot switch branches on Cloud instance {}/{}: instance is not linked to a project", org, inst)
                )
            }
        }
    }

    pub async fn get_project_config(&self) -> anyhow::Result<Option<Config>> {
        let project_dir = get_project_path(None, true).await?;
        let Some(path) = &project_dir else {
            return Ok(None);
        };
        Ok(Some(crate::portable::config::read(&path)?))
    }
}
