use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueHint;
use const_format::concatcp;
use gel_tokio::get_stash_path;

use crate::branding::BRANDING_CLOUD;
use crate::branding::{BRANDING_CLI_CMD, MANIFEST_FILE_DISPLAY_NAME};
use crate::commands::ExitCode;
use crate::portable::project;
use crate::print::{self, msg, Highlight};
use crate::table;

pub fn run(options: &Command) -> anyhow::Result<()> {
    let Some(project) = project::find_project(options.project_dir.as_deref())? else {
        anyhow::bail!("`{MANIFEST_FILE_DISPLAY_NAME}` not found, unable to get project info.");
    };
    let stash_dir = get_stash_path(&project.root)?;
    if !stash_dir.exists() {
        msg!(
            "{} {} Run `{BRANDING_CLI_CMD} project init`.",
            print::err_marker(),
            "Project is not initialized.".emphasized()
        );
        return Err(ExitCode::new(1).into());
    }
    let instance_name = fs::read_to_string(stash_dir.join("instance-name"))?;
    let cloud_profile_file = stash_dir.join("cloud-profile");
    let cloud_profile = cloud_profile_file
        .exists()
        .then(|| fs::read_to_string(cloud_profile_file))
        .transpose()?;

    let item = options
        .get
        .as_deref()
        .or(options.instance_name.then_some("instance-name"));
    if let Some(item) = item {
        match item {
            "instance-name" => {
                if options.json {
                    println!("{}", serde_json::to_string(&instance_name)?);
                } else {
                    println!("{instance_name}");
                }
            }
            "cloud-profile" => {
                if options.json {
                    println!("{}", serde_json::to_string(&cloud_profile)?);
                } else if let Some(profile) = cloud_profile {
                    println!("{profile}");
                }
            }
            _ => unreachable!(),
        }
    } else if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonInfo {
                instance_name: &instance_name,
                cloud_profile: cloud_profile.as_deref(),
                root: &project.root,
            })?
        );
    } else {
        let root = project.root.display().to_string();
        let mut rows: Vec<(&str, String)> =
            vec![("Instance name", instance_name), ("Project root", root)];
        if let Some(profile) = cloud_profile.as_deref() {
            rows.push((concatcp!(BRANDING_CLOUD, " profile"), profile.to_string()));
        }
        table::settings(rows.as_slice());
    }
    Ok(())
}

#[derive(clap::Args, Debug, Clone)]
pub struct Command {
    /// Explicitly set a root directory for the project
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Display only the instance name (shortcut to `--get instance-name`)
    #[arg(long)]
    pub instance_name: bool,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    #[arg(long, value_parser=[
        "instance-name",
        "cloud-profile",
    ])]
    /// Get a specific value:
    ///
    /// * `instance-name` -- Name of the listance the project is linked to
    pub get: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct JsonInfo<'a> {
    instance_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cloud_profile: Option<&'a str>,
    root: &'a Path,
}
