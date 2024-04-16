use crate::commands::parser::BranchingCmd;
use crate::options::ConnectionOptions;

#[derive(clap::Args, Debug, Clone)]
pub struct BranchCommand {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    Create(Create),
    Switch(Switch),
    List(List),
    Current(Current),
    Rebase(Rebase),
    Merge(Merge),
    Rename(Rename),
    Drop(Drop),
    Wipe(Wipe),
}

impl From<&BranchingCmd> for Command {
    fn from(cmd: &BranchingCmd) -> Self {
        match cmd {
            BranchingCmd::Create(args) => Command::Create(args.clone()),
            BranchingCmd::Drop(args) => Command::Drop(args.clone()),
            BranchingCmd::Wipe(args) => Command::Wipe(args.clone()),
        }
    }
}

/// Creates a new branch and switches to it.
#[derive(clap::Args, Debug, Clone)]
pub struct Create {
    /// The name of the branch to create.
    pub name: String,

    /// The optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// Create the branch without any schema or data.
    #[arg(short = 'e', long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// Copy data from the 'base' branch.
    #[arg(alias = "cp", long)]
    pub copy_data: bool,
}

/// Drops an existing branch, removing it and its data.
#[derive(clap::Args, Debug, Clone)]
pub struct Drop {
    /// The branch to drop.
    pub branch: String,

    /// Drop the branch without asking for confirmation.
    #[arg(long)]
    pub non_interactive: bool,

    /// Close any existing connections to the branch before dropping it.
    #[arg(long)]
    pub force: bool,
}

/// Wipes all data within a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Wipe {
    /// The branch to wipe.
    pub branch: String,

    /// Wipe without asking for confirmation.
    #[arg(long)]
    pub non_interactive: bool,
}

/// Switches the current branch to a different one.
#[derive(clap::Args, Debug, Clone)]
pub struct Switch {
    /// The branch to switch to.
    pub branch: String,

    /// Create the branch if it doesn't exist.
    #[arg(short = 'c', long)]
    pub create: bool,

    /// If creating a new branch: whether the new branch should be empty.
    #[arg(short = 'e', long, conflicts_with = "copy_data")]
    pub empty: bool,

    /// If creating a new branch: the optional 'base' of the branch to create.
    #[arg(long)]
    pub from: Option<String>,

    /// If creating a new branch: whether to copy data from the 'base' branch.
    #[arg(alias = "cp", long)]
    pub copy_data: bool,
}

/// Renames a branch.
#[derive(clap::Args, Debug, Clone)]
pub struct Rename {
    /// The branch to rename.
    pub old_name: String,

    /// The new name of the branch.
    pub new_name: String,

    /// Close any existing connection to the branch before renaming it.
    #[arg(long)]
    pub force: bool,
}

/// List all branches.
#[derive(clap::Args, Debug, Clone)]
pub struct List {}

/// Creates a new branch that is based on the target branch, but also contains any new migrations
/// on the current branch. Warning: data stored in current branch will be deleted.
#[derive(clap::Args, Debug, Clone)]
pub struct Rebase {
    /// The branch to rebase the current branch to.
    pub target_branch: String,

    /// Skip applying migrations generated from the rebase.
    #[arg(long)]
    pub no_apply: bool,
}

/// Merges a branch into this one via a fast-forward merge.
#[derive(clap::Args, Clone, Debug)]
pub struct Merge {
    /// The branch to merge into this one.
    pub target_branch: String,

    /// Skip applying migrations generated from the merge.
    #[arg(long)]
    pub no_apply: bool,
}

/// Prints the current branch.
#[derive(clap::Args, Clone, Debug)]
pub struct Current {
    /// Print as plain text output to stdout. Prints nothing instead of erroring if the current branch
    /// can't be resolved.
    #[arg(long)]
    pub plain: bool,
}
