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
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Init {
    /// Specifies a project root directory explicitly.
    #[clap(value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,
}
