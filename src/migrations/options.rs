use std::path::PathBuf;

use clap::{ValueHint};

use edgedb_cli_derive::{EdbClap, IntoArgs};

use crate::options::ConnectionOptions;
use crate::portable::repository::Channel;
use crate::portable::ver;


#[derive(EdbClap, Clone, Debug)]
pub struct Migration {
    #[clap(subcommand)]
    pub subcommand: MigrationCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum MigrationCmd {
    /// Apply migration from latest migration script
    Apply(Migrate),
    /// Create migration script inside /migrations
    Create(CreateMigration),
    /// Show current migration status
    Status(ShowStatus),
    /// Show all migration versions
    Log(MigrationLog),
    /// Edit migration file
    ///
    /// Invokes $EDITOR on the last migration file, and then fixes 
    /// migration id after editor exits. Usually should be used for
    /// migrations that have not been applied yet.
    Edit(MigrationEdit),
    /// Check if current schema is compatible with new EdgeDB version
    UpgradeCheck(UpgradeCheck),
}

#[derive(EdbClap, IntoArgs, Clone, Debug)]
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
    /// Squash all schema migrations into one and optionally provide a fixup migration.
    ///
    /// Note: this discards data migrations.
    #[clap(long)]
    pub squash: bool,
    /// Do not ask questions. By default works only if "safe" changes are
    /// to be done (those for which EdgeDB has a high degree of confidence).
    /// This safe default can be overridden with `--allow-unsafe`.
    #[clap(long)]
    pub non_interactive: bool,
    /// Apply the most probable unsafe changes in case there are ones. This
    /// is only useful in non-interactive mode.
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
    /// Do not print messages, only indicate success by exit status
    #[clap(long)]
    pub quiet: bool,

    /// Upgrade to a specified revision.
    ///
    /// A unique revision prefix can be specified instead of a full
    /// revision name.
    ///
    /// If this revision is applied, the command is a no-op. The command
    /// ensures that the revision is present, but additional applied revisions 
    /// are not considered an error.
    #[clap(long, conflicts_with="dev_mode")]
    pub to_revision: Option<String>,

    /// Apply current schema changes on top of those found in the migration history
    ///
    /// This is commonly used to apply schema temporarily before doing
    /// `migration create` for testing purposes.
    ///
    /// This works the same way as `edgedb watch` but without starting 
    /// a long-running watch task.
    #[clap(long)]
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
    /// (database connection not required)
    #[clap(long)]
    pub from_fs: bool,

    /// Print revisions from the database
    /// (no filesystem schema is required)
    #[clap(long)]
    pub from_db: bool,

    /// Sort migrations starting from newer to older, instead
    /// of the default older to newer
    #[clap(long)]
    pub newest_first: bool,

    /// Show maximum N revisions (default: no limit)
    #[clap(long)]
    pub limit: Option<usize>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct MigrationEdit {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Do not check migration using the database connection
    #[clap(long)]
    pub no_check: bool,
    /// Fix migration id non-interactively, and do not run editor
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, IntoArgs, Clone, Debug)]
pub struct UpgradeCheck {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Check upgrade to a specified version
    #[clap(long)]
    #[clap(conflicts_with_all=&[
        "to_testing", "to_nightly", "to_channel",
    ])]
    pub to_version: Option<ver::Filter>,

    /// Check upgrade to latest nightly version
    #[clap(long)]
    #[clap(conflicts_with_all=&[
        "to_version", "to_testing", "to_channel",
    ])]
    pub to_nightly: bool,

    /// Check upgrade to latest testing version
    #[clap(long)]
    #[clap(conflicts_with_all=&[
        "to_version", "to_nightly", "to_channel",
    ])]
    pub to_testing: bool,

    /// Check upgrade to latest version in the channel
    #[clap(long, value_enum)]
    #[clap(conflicts_with_all=&[
        "to_version", "to_nightly", "to_testing",
    ])]
    pub to_channel: Option<Channel>,

    /// Monitor schema changes and check again on change
    #[clap(long)]
    pub watch: bool,

    #[edb(hide=true)]
    pub run_server_with_status: Option<PathBuf>,
}
