use std::fmt;
use std::str::FromStr;

use clap::{AppSettings, ArgSettings, ValueHint};
use serde::{Serialize, Deserialize};
use edgedb_cli_derive::EdbClap;

use crate::server::version::Version;
use crate::server::methods::InstallMethod;
use crate::server::is_valid_name;


#[derive(EdbClap, Debug, Clone)]
pub struct ServerCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Debug, Clone)]
pub struct ServerInstanceCommand {
    #[clap(subcommand)]
    pub subcommand: InstanceCommand,
}

#[derive(EdbClap, Clone, Debug)]
pub enum InstanceCommand {
    /// Initialize a new EdgeDB instance
    Create(Create),
    /// Show all instances
    List(List),
    /// Show status of a matching instance
    Status(Status),
    /// Start an instance
    Start(Start),
    /// Stop an instance
    Stop(Stop),
    /// Restart an instance
    Restart(Restart),
    /// Destroy an instance and remove the data
    Destroy(Destroy),
    /// Link a remote instance
    #[edb(inherit(crate::options::ConnectionOptions))]
    Link(Link),
    /// Unlink a remote instance
    Unlink(Unlink),
    /// Show logs of an instance
    Logs(Logs),
    /// Upgrade installations and instances
    Upgrade(Upgrade),
    /// Revert a major instance upgrade
    Revert(Revert),
    /// Reset password for a user in the instance
    ResetPassword(ResetPassword),
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Show locally installed EdgeDB versions
    Info(Info),
    /// Install an EdgeDB version locally
    Install(Install),
    /// Uninstall an EdgeDB version locally
    Uninstall(Uninstall),
    /// List available and installed versions of EdgeDB
    ListVersions(ListVersions),
    /// Show system introspection debug info
    #[clap(name="_detect")]
    #[edb(hidden)]
    _Detect(Detect),
}

#[derive(EdbClap, Debug, Clone)]
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

#[derive(EdbClap, Debug, Clone)]
pub struct Uninstall {
    /// Uninstall all versions
    #[clap(long)]
    pub all: bool,
    /// Uninstall unused versions
    #[clap(long)]
    pub unused: bool,
    /// Uninstall nightly versions
    #[clap(long)]
    pub nightly: bool,
    /// Uninstall specific version
    #[clap(long, conflicts_with="nightly")]
    pub version: Option<Version<String>>,
    /// Increase verbosity
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Delete docker and package installations
    #[clap(long)]
    pub deprecated_install_methods: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct ListVersions {
    #[clap(long)]
    pub installed_only: bool,

    /// Single column output
    #[clap(long, possible_values=&[
        "major-version", "installed", "available",
    ])]
    pub column: Option<String>,

    /// Output in JSON format
    #[clap(long)]
    pub json: bool,

    /// Show versions of docker and package installations
    #[clap(long)]
    pub deprecated_install_methods: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StartConf {
    Auto,
    Manual,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Create {
    /// Name of the created instance
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
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

    /// Specifies whether to start EdgeDB automatically
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

    /// Overwrite data directory and credential file if any of these exist.
    /// This is mainly useful for recovering from interrupted initializations.
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

#[derive(EdbClap, Debug, Clone)]
pub struct Destroy {
    /// Name of the instance to destroy
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,
    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Force destroy even if instance is referred to by a project
    #[clap(long)]
    pub force: bool,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(long_about = "Link to a remote EdgeDB instance and
assign an instance name to simplify future connections.")]
pub struct Link {
    /// Specify a new instance name for the remote server. If not
    /// present, the name will be interactively asked.
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]
    pub name: Option<String>,

    /// Run in non-interactive mode (accepting all defaults)
    #[clap(long)]
    pub non_interactive: bool,

    /// Reduce command verbosity.
    #[clap(long)]
    pub quiet: bool,

    /// Trust peer certificate.
    #[clap(long)]
    pub trust_tls_cert: bool,

    /// Overwrite existing credential file if any.
    #[clap(long)]
    pub overwrite: bool,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(long_about = "Unlink from a remote EdgeDB instance.")]
pub struct Unlink {
    /// Specify the name of the remote instance.
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Start {
    /// Name of the instance to start
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,
    #[clap(long)]
    #[cfg_attr(target_os="linux",
        clap(about="Start the server in the foreground rather than using \
                    systemd to manage the process (note: you might need to \
                    stop non-foreground instance first)"))]
    #[cfg_attr(target_os="macos",
        clap(about="Start the server in the foreground rather than using \
                    launchctl to manage the process (note: you might need to \
                    stop non-foreground instance first)"))]
    pub foreground: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Stop {
    /// Name of the instance to stop
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Restart {
    /// Name of the instance to restart
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,
}

#[derive(EdbClap, Debug, Clone)]
pub struct List {
    /// Output more debug info about each instance
    #[clap(long, conflicts_with_all=&["debug", "json"])]
    pub extended: bool,

    /// Output all available debug info about each instance
    #[clap(long, setting=ArgSettings::Hidden)]
    #[clap(conflicts_with_all=&["extended", "json"])]
    pub debug: bool,

    /// Output in JSON format
    #[clap(long, conflicts_with_all=&["extended", "debug"])]
    pub json: bool,

    /// Show versions of docker and package installations
    #[clap(long)]
    pub deprecated_install_methods: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Status {
    /// Name of the instance
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,

    /// Show current systems service info
    #[clap(long, conflicts_with_all=&["debug", "json", "extended"])]
    pub service: bool,

    /// Output more debug info about each instance
    #[clap(long, conflicts_with_all=&["debug", "json", "service"])]
    pub extended: bool,

    /// Output all available debug info about each instance
    #[clap(long, setting=ArgSettings::Hidden)]
    #[clap(conflicts_with_all=&["extended", "json", "service"])]
    pub debug: bool,

    /// Output in JSON format
    #[clap(long, conflicts_with_all=&["extended", "debug", "service"])]
    pub json: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Logs {
    /// Name of the instance
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,

    /// Number of lines to show
    #[clap(short='n', long)]
    pub tail: Option<usize>,

    /// Show log's tail and the continue watching for the new entries
    #[clap(short='f', long)]
    pub follow: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Upgrade {
    /// Upgrade specified instance to the latest version
    #[clap(long, conflicts_with_all=&["to_nightly", "to_version"])]
    pub to_latest: bool,

    /// Upgrade specified instance to a specified version
    #[clap(long, conflicts_with_all=&["to_nightly", "to_latest"])]
    pub to_version: Option<Version<String>>,

    /// Upgrade specified instance to a latest nightly version
    #[clap(long, conflicts_with_all=&["to_version", "to_latest"])]
    pub to_nightly: bool,

    /// Instance to upgrade
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,

    /// Verbose output
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Force upgrade process even if there is no new version
    #[clap(long)]
    pub force: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Revert {
    /// Name of the instance to revert
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub name: String,

    /// Do not check if upgrade is in progress
    #[clap(long)]
    pub ignore_pid_check: bool,

    /// Do not ask for a confirmation
    #[clap(short='y', long)]
    pub no_confirm: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct ResetPassword {
    /// Name of the instance to reset
    #[clap(validator(instance_name_opt))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
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
    /// Save new user and password into a credentials file. By default
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

#[derive(EdbClap, Debug, Clone)]
pub struct Info {
    /// Display only the server binary path
    #[clap(long)]
    pub bin_path: bool,
    /// Output in JSON format
    #[clap(long)]
    pub json: bool,

    #[clap(long)]
    pub latest: bool,
    #[clap(long)]
    pub nightly: bool,
    #[clap(long, conflicts_with="nightly")]
    pub version: Option<Version<String>>,
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub method: Option<InstallMethod>,
}


#[derive(EdbClap, Debug, Clone)]
#[clap(setting=AppSettings::Hidden)]
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

pub fn instance_name_opt(name: &str) -> Result<(), String> {
    if is_valid_name(&name) {
        return Ok(())
    }
    return Err("instance name must be a valid identifier, \
                (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)".into())
}

