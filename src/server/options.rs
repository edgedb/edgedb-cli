use std::fmt;
use std::str::FromStr;
use clap::{Clap, AppSettings};

use crate::server::version::Version;
use crate::server::methods::InstallMethod;


#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ServerCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    #[clap(about="Install edgedb-server")]
    Install(Install),
    #[clap(about="List available and installed versions of the server")]
    ListVersions(ListVersions),
    #[clap(about="Initialize a new server instance")]
    Init(Init),
    #[clap(about="Start an instance")]
    Start(Start),
    #[clap(about="Stop an instance")]
    Stop(Stop),
    #[clap(about="Restart an instance")]
    Restart(Restart),
    #[clap(about="Status of an instance")]
    Status(Status),
    #[clap(name="_detect")]
    _Detect(Detect),
}

#[derive(Clap, Debug, Clone)]
pub struct Install {
    #[clap(short="i", long)]
    pub interactive: bool,
    #[clap(long)]
    pub nightly: bool,
    #[clap(long, conflicts_with="nightly")]
    pub version: Option<Version<String>>,
    #[clap(long, possible_values=&["package", "docker"][..])]
    pub method: Option<InstallMethod>,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::Hidden)]
pub struct ListVersions {
    #[clap(long)]
    pub installed_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StartConf {
    Auto,
    Manual,
}

#[derive(Clap, Debug, Clone)]
pub struct Init {
    #[clap(about="Database server instance name", default_value="default")]
    pub name: String,
    #[clap(long)]
    pub system: bool,
    #[clap(short="i", long)]
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
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Start {
    #[clap(about="Database server instance name", default_value="default")]
    pub name: String,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Stop {
    #[clap(about="Database server instance name", default_value="default")]
    pub name: String,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Restart {
    #[clap(about="Database server instance name", default_value="default")]
    pub name: String,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Status {
    #[clap(about="Database server instance name", default_value="default")]
    pub name: String,
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

