use std::fmt;
use std::str::FromStr;

use clap::{Clap, AppSettings, ArgSettings};
use serde::{Serialize, Deserialize};

use crate::server::version::Version;
use crate::server::methods::InstallMethod;
use crate::server::is_valid_name;


#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ServerCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(Clap, Clone, Debug)]
pub enum InstanceCommand {
    #[clap(about="Start an instance")]
    Start(Start),
    #[clap(about="Stop an instance")]
    Stop(Stop),
    #[clap(about="Restart an instance")]
    Restart(Restart),
    #[clap(about="Status of an instance")]
    Status(Status),
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    #[clap(about="Install edgedb-server")]
    Install(Install),
    #[clap(about="List available and installed versions of the server")]
    ListVersions(ListVersions),
    #[clap(about="Initialize a new server instance")]
    Init(Init),
    #[clap(about="Destroy a server instance and remove the data stored")]
    Destroy(Destroy),
    #[clap(flatten)]
    Instance(InstanceCommand),
    #[clap(about="Upgrade installations and instances")]
    Upgrade(Upgrade),
    #[clap(about="Reset password for a user in the instance")]
    ResetPassword(ResetPassword),
    #[clap(name="_detect")]
    _Detect(Detect),
}

#[derive(Clap, Debug, Clone)]
pub struct Install {
    #[clap(short='i', long)]
    pub interactive: bool,
    #[clap(long)]
    pub nightly: bool,
    #[clap(long, conflicts_with="nightly")]
    pub version: Option<Version<String>>,
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub method: Option<InstallMethod>,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListVersions {
    #[clap(long)]
    pub installed_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StartConf {
    Auto,
    Manual,
}

#[derive(Clap, Debug, Clone)]
pub struct Init {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,
    #[clap(long)]
    pub system: bool,
    #[clap(short='i', long)]
    pub interactive: bool,
    #[clap(long)]
    pub nightly: bool,
    #[clap(long, conflicts_with="nightly")]
    pub version: Option<Version<String>>,
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub method: Option<InstallMethod>,
    #[clap(long)]
    pub port: Option<u16>,
    #[clap(long, default_value="auto",
           possible_values=&["auto", "manual"][..])]
    pub start_conf: StartConf,

    /// Default database name (created during initialization, and saved in
    /// credentials file)
    #[clap(long, default_value="edgedb")]
    pub default_database: String,
    /// Default user name (created during initialization, and saved in
    /// credentials file)
    #[clap(long, default_value="edgedb")]
    pub default_user: String,

    /// Overwrite data directory and credential file if any of these exists.
    /// This is mainly useful for recovering from interruped initializations.
    #[clap(long)]
    pub overwrite: bool,

    /// Do not create a user and database named after current unix user
    #[clap(long, setting=ArgSettings::Hidden)]
    pub inhibit_user_creation: bool,
    /// Do not start database right now, even if --start-conf=auto
    #[clap(long, setting=ArgSettings::Hidden)]
    pub inhibit_start: bool,
    /// Write upgrade metadata marker with specified contents
    #[clap(long, setting=ArgSettings::Hidden)]
    pub upgrade_marker: Option<String>,
}

#[derive(Clap, Debug, Clone)]
pub struct Destroy {
    /// Database server instance name to destroy
    #[clap(validator(instance_name_opt))]
    pub name: String,
    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Start {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,
    #[clap(long)]
    #[cfg_attr(target_os="linux",
        clap(about="Start the server in the foreground rather than using \
                    systemd to manage the process (note you might need to \
                    stop non-foreground instance first)"))]
    #[cfg_attr(target_os="macos",
        clap(about="Start the server in the foreground rather than using \
                    launchctl to manage the process (note you might need to \
                    stop non-foreground instance first)"))]
    pub foreground: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Stop {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Restart {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Status {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,

    /// Show current systems service info
    #[clap(long)]
    pub service: bool,

    /// Output more debug info about each instance
    #[clap(long)]
    pub extended: bool,

    /// Output all available debug info about each instance
    #[clap(long, setting=ArgSettings::Hidden)]
    pub debug: bool,

    /// Print status of all instances
    #[clap(long)]
    pub all: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion, after_help="\
There are few modes of operation of this command:

edgedb server upgrade
  Without arguments this command upgrades all instances which aren't running
  nightly EdgeDB to a latest minor version of the server.

edgedb server upgrade <name> [--to-version=<ver>|--to-nightly]
  Upgrades specified instance to the specified major version of the server or
  to the latest nightly, by default upgrades to the latest stable. This only
  works for instances that initially aren't running nightly.

edgedb server upgrade --nightly
  Upgrades all existing nightly instances to the latest EdgeDB nightly.
")]
pub struct Upgrade {
    /// Upgrade all nightly instances
    #[clap(long)]
    pub nightly: bool,

    /// Upgrade specified instance(s) to a specified major version
    #[clap(long)]
    pub to_version: Option<Version<String>>,

    /// Upgrade specifies instance to a latest nightly version
    #[clap(long)]
    pub to_nightly: bool,

    /// Only upgrade specicified database instance
    pub name: Option<String>,

    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Force upgrade process even if there is no new version
    #[clap(long)]
    pub force: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ResetPassword {
    /// Database server instance name
    #[clap(default_value="default", validator(instance_name_opt))]
    pub name: String,
    /// User to change password for. Default is got from credentials file.
    #[clap(long)]
    pub user: Option<String>,
    /// Read a password from the terminal rather than generating new one
    #[clap(long)]
    pub password: bool,
    /// Read a password from stdin rather than generating new one
    #[clap(long)]
    pub password_from_stdin: bool,
    /// Save new user and password password into a credentials file. By default
    /// credentials file is updated only if user name matches.
    #[clap(long)]
    pub save_credentials: bool,
    /// Do not save generated password into a credentials file even if user name matches.
    #[clap(long)]
    pub no_save_credentials: bool,
    /// Do not print any messages, only indicate success by exit status
    #[clap(long)]
    pub quiet: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::Hidden)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Detect {
}

impl FromStr for StartConf {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<StartConf> {
        match s {
            "auto" => Ok(StartConf::Auto),
            "manual" => Ok(StartConf::Manual),
            _ => anyhow::bail!("Unsupported start configuration, \
                options: `auto`, `manual`"),
        }
    }
}

impl StartConf {
    fn as_str(&self) -> &str {
        match self {
            StartConf::Auto => "auto",
            StartConf::Manual => "manual",
        }
    }
}

impl fmt::Display for StartConf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

fn instance_name_opt(name: &str) -> Result<(), String> {
    if is_valid_name(&name) {
        return Ok(())
    }
    return Err("instance name must be a valid identifier, \
                (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)".into())
}

