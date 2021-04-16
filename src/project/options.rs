use std::path::PathBuf;

use clap::{Clap, AppSettings, ValueHint};


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
