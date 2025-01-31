pub mod info;
pub mod init;
pub mod manifest;
pub mod unlink;
pub mod upgrade;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use fn_error_context::context;

use gel_tokio::Builder;

use crate::branding::QUERY_TAG;
use crate::branding::{BRANDING_SCHEMA_FILE_EXT, MANIFEST_FILE_DISPLAY_NAME};
use crate::cloud::client::CloudClient;
use crate::connect::Connection;
use crate::platform::{bytes_to_path, path_bytes};
use crate::platform::{config_dir, is_schema_file, symlink_dir, tmp_file_path};
use crate::portable::local::InstanceInfo;
use crate::portable::options::InstanceName;
use crate::portable::repository::Query;
use crate::portable::ver;
use crate::print;
use crate::print::AsRelativeToCurrentDir;

pub fn run(cmd: &Command, options: &crate::options::Options) -> anyhow::Result<()> {
    use crate::portable::project::Subcommands::*;

    match &cmd.subcommand {
        Init(c) => init::run(c, options),
        Unlink(c) => unlink::run(c, options),
        Info(c) => info::run(c),
        Upgrade(c) => upgrade::run(c, options),
    }
}

#[derive(clap::Args, Debug, Clone)]
#[command(version = "help_expand")]
#[command(disable_version_flag = true)]
pub struct Command {
    #[command(subcommand)]
    pub subcommand: Subcommands,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Subcommands {
    /// Initialize project or link to existing unlinked project
    Init(init::Command),
    /// Clean up project configuration.
    ///
    /// Use [`BRANDING_CLI_CMD`] project init to relink.
    Unlink(unlink::Command),
    /// Get various metadata about project instance
    Info(info::Command),
    /// Upgrade [`BRANDING`] instance used for current project
    ///
    /// Data is preserved using a dump/restore mechanism.
    ///
    /// Upgrades to version specified in `{gel,edgedb}.toml` unless other options specified.
    ///
    /// Note: May fail if lower version is specified (e.g. moving from nightly to stable).
    Upgrade(upgrade::Command),
}

const DEFAULT_SCHEMA: &str = "\
    module default {\n\
    \n\
    }\n\
";

const FUTURES_SCHEMA: &str = "\
    # Disable the application of access policies within access policies\n\
    # themselves. This behavior will become the default in EdgeDB 3.0.\n\
    # See: https://www.edgedb.com/docs/reference/ddl/access_policies#nonrecursive\n\
    using future nonrecursive_access_policies;\n\
";

const SIMPLE_SCOPING_SCHEMA: &str = "\
    # Use a simpler algorithm for resolving the scope of object names.\n\
    # This behavior will become the default in Gel 7.0.\n\
    # See: https://docs.edgedb.com/database/edgeql/path_resolution#new-path-scoping\n\
    using future simple_scoping;\n\
";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectInfo {
    instance_name: String,
    stash_dir: PathBuf,
}

pub struct Handle<'a> {
    name: String,
    instance: InstanceKind<'a>,
    project_dir: PathBuf,
    schema_dir: PathBuf,
    database: Option<String>,
}

pub struct StashDir<'a> {
    project_dir: &'a Path,
    instance_name: &'a str,
    database: Option<&'a str>,
    cloud_profile: Option<&'a str>,
}

pub enum InstanceKind<'a> {
    Remote,
    Portable(InstanceInfo),
    Wsl,
    Cloud {
        org_slug: String,
        name: String,
        cloud_client: &'a CloudClient,
    },
}

pub fn stash_base() -> anyhow::Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

impl<'a> StashDir<'a> {
    fn new(project_dir: &'a Path, instance_name: &'a str) -> StashDir<'a> {
        StashDir {
            project_dir,
            instance_name,
            database: None,
            cloud_profile: None,
        }
    }
    #[context("error writing project dir {:?}", dir)]
    fn write(&self, dir: &Path) -> anyhow::Result<()> {
        let tmp = tmp_file_path(dir);
        fs::create_dir_all(&tmp)?;
        fs::write(tmp.join("project-path"), path_bytes(self.project_dir)?)?;
        fs::write(tmp.join("instance-name"), self.instance_name.as_bytes())?;
        if let Some(profile) = self.cloud_profile {
            fs::write(tmp.join("cloud-profile"), profile.as_bytes())?;
        }
        if let Some(database) = &self.database {
            fs::write(tmp.join("database"), database.as_bytes())?;
        }

        let lnk = tmp.join("project-link");
        symlink_dir(self.project_dir, &lnk)
            .map_err(|e| {
                log::info!("Error symlinking project at {:?}: {}", lnk, e);
            })
            .ok();
        fs::rename(&tmp, dir)?;
        Ok(())
    }
}

impl InstanceKind<'_> {
    fn is_local(&self) -> bool {
        match self {
            InstanceKind::Wsl => true,
            InstanceKind::Portable(_) => true,
            InstanceKind::Remote => false,
            InstanceKind::Cloud { .. } => false,
        }
    }
}

impl Handle<'_> {
    pub fn probe<'a>(
        name: &InstanceName,
        project_dir: &Path,
        schema_dir: &Path,
        cloud_client: &'a CloudClient,
    ) -> anyhow::Result<Handle<'a>> {
        match name {
            InstanceName::Local(name) => match InstanceInfo::try_read(name)? {
                Some(info) => Ok(Handle {
                    name: name.into(),
                    instance: InstanceKind::Portable(info),
                    project_dir: project_dir.into(),
                    schema_dir: schema_dir.into(),
                    database: None,
                }),
                None => Ok(Handle {
                    name: name.into(),
                    instance: InstanceKind::Remote,
                    project_dir: project_dir.into(),
                    schema_dir: schema_dir.into(),
                    database: None,
                }),
            },
            InstanceName::Cloud {
                org_slug,
                name: inst_name,
            } => Ok(Handle {
                name: name.to_string(),
                instance: InstanceKind::Cloud {
                    org_slug: org_slug.to_owned(),
                    name: inst_name.to_owned(),
                    cloud_client,
                },
                database: None,
                project_dir: project_dir.into(),
                schema_dir: schema_dir.into(),
            }),
        }
    }
    pub fn get_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::new();
        builder.instance(&self.name)?;
        if let Some(database) = &self.database {
            builder.database(database)?;
        }
        Ok(builder)
    }
    pub fn get_default_builder(&self) -> anyhow::Result<Builder> {
        let mut builder = Builder::new();
        builder.instance(&self.name)?;
        Ok(builder)
    }
    pub async fn get_default_connection(&self) -> anyhow::Result<Connection> {
        Ok(Connection::connect(&self.get_default_builder()?.build_env().await?, QUERY_TAG).await?)
    }
    pub async fn get_connection(&self) -> anyhow::Result<Connection> {
        Ok(Connection::connect(&self.get_builder()?.build_env().await?, QUERY_TAG).await?)
    }
    #[tokio::main(flavor = "current_thread")]
    pub async fn get_version(&self) -> anyhow::Result<ver::Build> {
        let mut conn = self.get_default_connection().await?;
        anyhow::Ok(conn.get_version().await?.clone())
    }
    fn check_version(&self, ver_query: &Query) {
        match self.get_version() {
            Ok(inst_ver) if ver_query.matches(&inst_ver) => {}
            Ok(inst_ver) => {
                print::warn!(
                    "WARNING: existing instance has version {}, \
                    but {} is required by {MANIFEST_FILE_DISPLAY_NAME}",
                    inst_ver,
                    ver_query.display()
                );
            }
            Err(e) => {
                log::warn!("Could not check instance's version: {:#}", e);
            }
        }
    }
}

#[context("cannot read schema directory `{}`", path.as_relative().display())]
fn find_schema_files(path: &Path) -> anyhow::Result<bool> {
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let is_schema_file = entry
            .file_name()
            .to_str()
            .map(is_schema_file)
            .unwrap_or(false);
        if is_schema_file {
            return Ok(true);
        }
    }
    return Ok(false);
}

#[context("cannot create default schema in `{}`", dir.as_relative().display())]
fn write_schema_default(dir: &Path, version: &Query) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    fs::create_dir_all(dir.join("migrations"))?;
    let default = dir.join(format!("default.{BRANDING_SCHEMA_FILE_EXT}"));
    let tmp = tmp_file_path(&default);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, DEFAULT_SCHEMA)?;
    fs::rename(&tmp, &default)?;

    if version.is_nonrecursive_access_policies_needed() {
        let futures = dir.join(format!("futures.{BRANDING_SCHEMA_FILE_EXT}"));
        let tmp = tmp_file_path(&futures);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, FUTURES_SCHEMA)?;
        fs::rename(&tmp, &futures)?;
    };
    if version.is_simple_scoping_needed() {
        let futures = dir.join(format!("scoping.{BRANDING_SCHEMA_FILE_EXT}"));
        let tmp = tmp_file_path(&futures);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, SIMPLE_SCOPING_SCHEMA)?;
        fs::rename(&tmp, &futures)?;
    };
    Ok(())
}

#[context("cannot read instance name of {:?}", stash_dir)]
pub fn instance_name(stash_dir: &Path) -> anyhow::Result<InstanceName> {
    let inst = fs::read_to_string(stash_dir.join("instance-name"))?;
    InstanceName::from_str(inst.trim())
}

#[context("cannot read database name of {:?}", stash_dir)]
pub fn database_name(stash_dir: &Path) -> anyhow::Result<Option<String>> {
    let inst = match fs::read_to_string(stash_dir.join("database")) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e)?,
    };
    Ok(Some(inst.trim().into()))
}

#[derive(Clone)]
pub struct Context {
    pub location: Location,
    pub manifest: manifest::Manifest,
}

#[derive(Clone)]
pub struct Location {
    pub root: PathBuf,
    pub manifest: PathBuf,
}

pub async fn find_project_async(override_dir: Option<&Path>) -> anyhow::Result<Option<Location>> {
    let manifest = gel_tokio::get_project_path(override_dir, true).await?;

    Ok(manifest.map(|manifest| Location {
        root: manifest.parent().unwrap().to_owned(),
        manifest,
    }))
}

#[tokio::main(flavor = "current_thread")]
pub async fn find_project(override_dir: Option<&Path>) -> anyhow::Result<Option<Location>> {
    find_project_async(override_dir).await
}

pub async fn load_ctx(override_dir: Option<&Path>) -> anyhow::Result<Option<Context>> {
    let Some(location) = find_project_async(override_dir).await? else {
        return Ok(None);
    };

    let manifest = manifest::read(&location.manifest)?;
    Ok(Some(Context { location, manifest }))
}

#[tokio::main(flavor = "current_thread")]
pub async fn load_ctx_at(location: Location) -> anyhow::Result<Context> {
    let manifest = manifest::read(&location.manifest)?;
    Ok(Context { location, manifest })
}

#[tokio::main(flavor = "current_thread")]
pub async fn ensure_ctx(override_dir: Option<&Path>) -> anyhow::Result<Context> {
    let Some(ctx) = load_ctx(override_dir).await? else {
        return Err(anyhow::anyhow!("`{MANIFEST_FILE_DISPLAY_NAME}` not found, unable to perform this action without an initialized project."));
    };

    Ok(ctx)
}

impl Context {
    pub fn resolve_schema_dir(&self) -> anyhow::Result<PathBuf> {
        self.manifest
            .project()
            .resolve_schema_dir(&self.location.root)
    }
}

pub fn find_project_dirs_by_instance(name: &str) -> anyhow::Result<Vec<PathBuf>> {
    find_project_stash_dirs("instance-name", |val| name == val, true)
        .map(|projects| projects.into_values().flatten().collect())
}

#[context("could not read project dir {:?}", stash_base())]
pub fn find_project_stash_dirs(
    get: &str,
    f: impl Fn(&str) -> bool,
    verbose: bool,
) -> anyhow::Result<HashMap<String, Vec<PathBuf>>> {
    let mut res = HashMap::new();
    let dir = match fs::read_dir(stash_base()?) {
        Ok(dir) => dir,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(res);
        }
        Err(e) => return Err(e)?,
    };
    for item in dir {
        let entry = item?;
        let sub_dir = entry.path();
        if sub_dir
            .file_name()
            .and_then(|f| f.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(true)
        {
            // skip hidden files, most likely .DS_Store (see #689)
            continue;
        }
        let path = sub_dir.join(get);
        let value = match fs::read_to_string(&path) {
            Ok(value) => value.trim().to_string(),
            Err(e) => {
                if verbose {
                    log::warn!("Error reading {:?}: {}", path, e);
                }
                continue;
            }
        };
        if f(&value) {
            res.entry(value).or_default().push(entry.path());
        }
    }
    Ok(res)
}

pub fn print_instance_in_use_warning(name: &str, project_dirs: &[PathBuf]) {
    print::warn!(
        "Instance {:?} is used by the following project{}:",
        name,
        if project_dirs.len() > 1 { "s" } else { "" }
    );
    for dir in project_dirs {
        let dest = match read_project_path(dir) {
            Ok(path) => path,
            Err(e) => {
                print::error!("{e}");
                continue;
            }
        };
        eprintln!("  {}", dest.as_relative().display());
    }
}

#[context("cannot read {:?}", project_dir)]
pub fn read_project_path(project_dir: &Path) -> anyhow::Result<PathBuf> {
    let bytes = fs::read(project_dir.join("project-path"))?;
    Ok(bytes_to_path(&bytes)?.to_path_buf())
}
