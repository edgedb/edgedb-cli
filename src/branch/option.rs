#[derive(clap::Args, Debug, Clone)]
pub struct BranchCommand {
    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    Create(Create),
    Drop(Drop),
    Wipe(Wipe),
    Switch(Switch),
    Rename(Rename),
    List(List),
}

#[derive(clap::Args, Debug, Clone)]
pub struct Create {
    pub branch: String,

    #[arg(long)]
    pub from: Option<String>,

    #[arg(long, conflicts_with = "copy_data")]
    pub empty: bool,

    #[arg(long)]
    pub copy_data: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Drop {
    pub branch: String,

    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Wipe {
    pub branch: String,

    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Switch {
    pub branch: String,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Rename {
    pub old_name: String,
    pub new_name: String,
}
#[derive(clap::Args, Debug, Clone)]
pub struct List {}
