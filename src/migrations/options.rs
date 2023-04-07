use std::path::PathBuf;

use clap::{ValueHint};

use edgedb_cli_derive::EdbClap;

use crate::options::ConnectionOptions;


#[derive(EdbClap, Clone, Debug)]
pub struct Migration {
    #[clap(subcommand)]
    pub subcommand: MigrationCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum MigrationCmd {
    /// Bring current database to the latest or a specified revision
    Apply(Migrate),
    /// Create a migration script
    Create(CreateMigration),
    /// Show current migration state
    Status(ShowStatus),
    /// Show all migration versions
    Log(MigrationLog),
    /// Edit migration file
    ///
    /// Invokes $EDITOR on the last migration file, and then fixes migration id
    /// after editor exits. Usually should be used for
    /// migrations that haven't been applied yet.
    Edit(MigrationEdit),
}

#[derive(EdbClap, Clone, Debug)]
pub struct MigrationConfig {
    /// Directory where `*.esdl` and `*.edgeql` files are located.
    /// Default is `./dbschema`
    #[clap(long, value_hint=ValueHint::DirPath)]
    pub schema_dir: Option<PathBuf>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct CreateMigration {
    #[clap(flatten)]
    pub cfg: MigrationConfig,
    /// Squash all migrations into one and optionally provide fixup migration.
    ///
    /// Note: this discards data migrations.
    #[clap(long)]
    pub squash: bool,
    /// Do not ask questions. By default works only if "safe" changes are
    /// to be done. Unless `--allow-unsafe` is also specified.
    #[clap(long)]
    pub non_interactive: bool,
    /// Apply the most probable unsafe changes in case there are ones. This
    /// is only useful in non-interactive mode
    #[clap(long)]
    pub allow_unsafe: bool,
    /// Create a new migration even if there are no changes (use this for
    /// data-only migrations)
    #[clap(long)]
    pub allow_empty: bool,
    /// Print queries executed
    #[clap(long, hide=true)]
    pub debug_print_queries: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Migrate {
    #[clap(flatten)]
    pub cfg: MigrationConfig,
    /// Do not print any messages, only indicate success by exit status
    #[clap(long)]
    pub quiet: bool,

    /// Upgrade to a specified revision.
    ///
    /// Unique prefix of the revision can be specified instead of full
    /// revision name.
    ///
    /// If this revision is applied, the command is no-op. The command
    /// ensures that this revision present, but it's not an error if more
    /// revisions are applied on top.
    #[clap(long, conflicts_with="dev_mode")]
    pub to_revision: Option<String>,

    /// Apply current schema changes on top of what's in the migration history
    ///
    /// This is commonly used to apply schema temporarily before doing
    /// `migration create` for testing purposes.
    #[clap(long, hide=true)]
    pub dev_mode: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ShowStatus {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Do not print any messages, only indicate success by exit status
    #[clap(long)]
    pub quiet: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct MigrationLog {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Print revisions from the filesystem
    /// (doesn't require database connection)
    #[clap(long)]
    pub from_fs: bool,

    /// Print revisions from the database
    /// (no filesystem schema is required)
    #[clap(long)]
    pub from_db: bool,

    /// Sort migrations starting from newer to older,
    /// by default older revisions go first
    #[clap(long)]
    pub newest_first: bool,

    /// Show maximum N revisions (default is unlimited)
    #[clap(long)]
    pub limit: Option<usize>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct MigrationEdit {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Do not check migration within the database connection
    #[clap(long)]
    pub no_check: bool,
    /// Fix migration id non-interactively, and don't run editor
    #[clap(long)]
    pub non_interactive: bool,
}
