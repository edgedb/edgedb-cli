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
    Rebase(Rebase)
}

/// Creates a new branch and switches to it.
#[derive(clap::Args, Debug, Clone)]
pub struct Create {
    /// The name of the branch to create.
    pub branch: String,

    /// The optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// Whether or not the new branch should contain no data.
    #[arg(long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// Whether or not to copy data from the 'base' branch.
    #[arg(long)]
    pub copy_data: bool,
}

/// Drops an existing branch, removing it and it's data.
#[derive(clap::Args, Debug, Clone)]
pub struct Drop {
    /// The branch to drop.
    pub branch: String,

    /// Whether or not to drop the branch non-interactively.
    #[arg(long)]
    pub non_interactive: bool,

    /// Whether or not to force drop the branch, this will close any existing connections to the branch before dropping
    /// it.
    #[arg(long)]
    pub force: bool,
}

/// Wipes all data within a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Wipe {
    /// The branch to wipe.
    pub branch: String,

    /// Whether or not to wipe it non-interactively.
    #[arg(long)]
    pub non_interactive: bool,
}

/// Switches the current branch to a different one.
#[derive(clap::Args, Debug, Clone)]
pub struct Switch {
    /// The branch to switch to.
    pub branch: String,
}

/// Renames a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Rename {
    /// The branch to rename.
    pub old_name: String,

    /// The new name of the branch.
    pub new_name: String,

    /// Whether or not to force rename the branch, this will close any existing connection to the branch before renaming
    /// it.
    #[arg(long)]
    pub force: bool,
}

/// Lists all branches.
#[derive(clap::Args, Debug, Clone)]
pub struct List {}

/// Creates a new branch that is based on the target branch, but also
/// contains any new migrations on the current branch.
/// Warning: data stored in current branch will be deleted.
#[derive(clap::Args, Debug, Clone)]
pub struct Rebase {
    /// The branch to rebase the current branch to.
    pub target_branch: String,

    /// Whether or not to apply migrations generated from the rebase.
    #[arg(long)]
    pub no_apply: bool,
}
