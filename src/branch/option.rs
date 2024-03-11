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
    pub name: String,

    /// The optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// Whether the new branch should contain no data.
    #[arg(short='e', long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// Whether to copy data from the 'base' branch.
    #[arg(alias="cp", long)]
    pub copy_data: bool,
}

/// Drops an existing branch, removing it and its data.
#[derive(clap::Args, Debug, Clone)]
pub struct Drop {
    /// The branch to drop.
    pub branch: String,

    /// Whether to drop the branch non-interactively.
    #[arg(long)]
    pub non_interactive: bool,

    /// Whether to force drop the branch, this will close any existing connections to the branch
    /// before dropping it.
    #[arg(long)]
    pub force: bool,
}

/// Wipes all data within a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Wipe {
    /// The branch to wipe.
    pub branch: String,

    /// Whether to wipe it non-interactively.
    #[arg(long)]
    pub non_interactive: bool,
}

/// Switches the current branch to a different one.
#[derive(clap::Args, Debug, Clone)]
pub struct Switch {
    /// The branch to switch to.
    pub branch: String,

    /// Whether to create the branch if it doesn't exist.
    #[arg(short='c', long)]
    pub create: bool,

    /// If creating a new branch: whether the new branch should be empty.
    #[arg(short='e', long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// If creating a new branch: the optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// If creating a new branch: whether to copy data from the 'base' branch.
    #[arg(alias="cp", long)]
    pub copy_data: bool,
}

/// Renames a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Rename {
    /// The branch to rename.
    pub old_name: String,

    /// The new name of the branch.
    pub new_name: String,

    /// Whether to force rename the branch, this will close any existing connection to the branch 
    /// before renaming it.
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

    /// Whether to apply migrations generated from the rebase, by default, all generated migrations
    /// are applied automatically.
    #[arg(long)]
    pub no_apply: bool,
}
