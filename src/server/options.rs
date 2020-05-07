use clap::{Clap, AppSettings};


#[derive(Clap, Debug, Clone)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ServerCommand {
    #[clap(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    Install(Install),
}

#[derive(Clap, Debug, Clone)]
pub struct Install {
    #[clap(short="i", long)]
    pub interactive: bool,
}
