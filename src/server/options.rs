use clap::{Clap, AppSettings};

use crate::server::version::Version;
use crate::server::install::InstallMethod;


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
    ListVersions(ListVersions),
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
    installed_only: bool,
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::Hidden)]
pub struct Detect {
}
