use gel_tokio::get_stash_path;

use crate::branding::BRANDING_CLOUD;
use crate::connect::Connection;
use crate::credentials;
use crate::platform::tmp_file_path;
use crate::portable::options::InstanceName;
use crate::portable::project;
use std::fs;
use std::sync::Mutex;

pub struct Context {
    /// Instance name provided either with --instance or inferred from the project.
    instance_name: Option<InstanceName>,

    /// Project location if --instance was not specified and
    /// current directory is within a project.
    project: Option<project::Location>,

    /// None means that the current branch is unknown because:
    /// - the instance uses the default branch (and we cannot know what
    ///   that is without making a query), or
    /// - we don't know which instance we are connecting to. This might be because:
    ///   - there was neither a project or the --instance option,
    ///   - the project has no linked instance.
    current_branch: Option<String>,

    /// Project manifest cache
    manifest_cache: Mutex<Option<Option<project::Context>>>,
}

impl Context {
    pub async fn new(instance_arg: Option<&InstanceName>) -> anyhow::Result<Context> {
        let mut ctx = Context {
            instance_name: None,
            current_branch: None,
            project: None,
            manifest_cache: Mutex::new(None),
        };

        // use instance name provided with --instance
        ctx.instance_name = instance_arg.cloned();
        if let Some(instance_name) = &ctx.instance_name {
            let instance_name = ensure_local_instance(instance_name)?;

            let credentials_path = credentials::path(instance_name)?;
            if credentials_path.exists() {
                let credentials = credentials::read(&credentials_path).await?;
                ctx.current_branch = credentials.branch.or(credentials.database);
            }
            return Ok(ctx);
        }

        // find the project and use it's instance name and branch
        ctx.project = project::find_project_async(None).await?;
        if let Some(location) = &ctx.project {
            let stash_dir = get_stash_path(&location.root)?;
            ctx.instance_name = project::instance_name(&stash_dir).ok();
            ctx.current_branch = project::database_name(&stash_dir).ok().flatten();
        }

        Ok(ctx)
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

        if let Some(project) = &self.project {
            // only place to store the branch is the database file in the project
            let stash_path = get_stash_path(&project.root)?.join("database");

            // ensure that the temp file is created in the same directory as the 'database' file
            let tmp = tmp_file_path(&stash_path);
            fs::write(&tmp, branch)?;
            fs::rename(&tmp, &stash_path)?;
        } else {
            let name = ensure_local_instance(instance_name)?;

            let path = credentials::path(name)?;
            let mut credentials = credentials::read(&path).await?;
            credentials.database = Some(branch.to_string());
            credentials.branch = Some(branch.to_string());

            credentials::write_async(&path, &credentials).await?;
        }
        Ok(())
    }

    pub async fn get_project(&self) -> anyhow::Result<Option<project::Context>> {
        if let Some(mani) = &*self.manifest_cache.lock().unwrap() {
            return Ok(mani.clone());
        }

        let manifest = project::load_ctx(None).await?;

        let mut cache_lock = self.manifest_cache.lock().unwrap();
        *cache_lock = Some(manifest.clone());
        Ok(manifest)
    }
}

fn ensure_local_instance(instance_name: &InstanceName) -> anyhow::Result<&str> {
    match instance_name {
        InstanceName::Local(instance) => Ok(instance),
        InstanceName::Cloud { .. } => {
            // should never occur because of the above check
            Err(anyhow::anyhow!(
                "cannot use branches on {BRANDING_CLOUD} instance unless linked to a project"
            ))
        }
    }
}
