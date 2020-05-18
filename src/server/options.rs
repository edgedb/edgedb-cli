use clap::{Clap, AppSettings};

use crate::server::version::Version;


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
}

#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::Hidden)]
pub struct Detect {
}
