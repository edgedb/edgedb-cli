use std::path::PathBuf;

use clap::{Clap, AppSettings, ValueHint};
use crate::server::methods::InstallMethod;
use crate::server::version::Version;
use crate::server::options::instance_name_opt;


#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ProjectCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    /// Initialize a new or existing project
    Init(Init),
    /// Remove association with and optionally destroy the
    /// linked EdgeDB intstance.
    Unlink(Unlink),
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Init {
    /// Specifies a project root directory explicitly.
    #[clap(value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Specifies the desired EdgeDB server version
    #[clap(long)]
    pub server_version: Option<Version<String>>,

    /// Specifies the EdgeDB server instance to be associated with the project
    #[clap(long, validator(instance_name_opt))]
    pub server_instance: Option<String>,

    /// Specifies a project root directory explicitly.
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub server_install_method: Option<InstallMethod>,

    /// Run in non-interactive mode (accepting all defaults)
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Unlink {
    /// Specifies a project root directory explicitly.
    #[clap(value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// If specified, the associated EdgeDB instance is destroyed by running edgedb server destroy.
    #[clap(long, short='D')]
    pub destroy_server_instance: bool,

    #[clap(long)]
    pub non_interactive: bool,
}
