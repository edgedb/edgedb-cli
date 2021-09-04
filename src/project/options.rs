use std::path::PathBuf;

use clap::{ValueHint};
use edgedb_cli_derive::EdbClap;

use crate::server::methods::InstallMethod;
use crate::server::version::Version;
use crate::server::options::instance_name_opt;


#[derive(EdbClap, Debug, Clone)]
pub struct ProjectCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Initialize a new or existing project
    Init(Init),
    /// Link existing server instance with current project
    Link(Link),
    /// Clean-up the project configuration
    Unlink(Unlink),
    /// Get various metadata about the project
    Info(Info),
    /// Upgrade EdgeDB instance used for the current project
    ///
    /// This command has two modes of operation.
    ///
    /// Upgrade instance to a version specified in `edgedb.toml`:
    ///
    ///     project upgrade
    ///
    /// Update `edgedb.toml` to a new version and upgrade the instance:
    ///
    ///     project upgrade --to-latest
    ///     project upgrade --to-version=1-beta2
    ///     project upgrade --to-nightly
    ///
    /// In all cases your data is preserved and converted using dump/restore
    /// mechanism. This might fail if lower version is specified (for example
    /// if upgrading from nightly to the stable version).
    Upgrade(Upgrade),
}

#[derive(EdbClap, Debug, Clone)]
pub struct Init {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Specifies the desired EdgeDB server version
    #[clap(long)]
    pub server_version: Option<Version<String>>,

    /// Specifies the EdgeDB server instance to be associated with the project
    #[clap(long, validator(instance_name_opt))]
    pub server_instance: Option<String>,

    /// Specifies which server should be used for this project: server
    /// installed via the local package system (package) or as a docker
    /// image (docker).
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub server_install_method: Option<InstallMethod>,

    /// Run in non-interactive mode (accepting all defaults)
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Unlink {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// If specified, the associated EdgeDB instance is destroyed by running
    /// `edgedb instance destroy`.
    #[clap(long, short='D')]
    pub destroy_server_instance: bool,

    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Link {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Specifies the EdgeDB server instance to be associated with the project.
    /// If not present, the name will be interactively asked.
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]
    pub name: Option<String>,

    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Info {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Display only the instance name
    #[clap(long)]
    pub instance_name: bool,

    /// Output in JSON format
    #[clap(long)]
    pub json: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Upgrade {
    /// Specifies a project root directory explicitly.
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub project_dir: Option<PathBuf>,

    /// Upgrade to a latest stable version
    #[clap(long)]
    pub to_latest: bool,

    /// Upgrade to a specified major version
    #[clap(long)]
    pub to_version: Option<Version<String>>,

    /// Upgrade to a latest nightly version
    #[clap(long)]
    pub to_nightly: bool,

    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Force upgrade process even if there is no new version
    #[clap(long)]
    pub force: bool,
}
