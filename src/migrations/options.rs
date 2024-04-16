use std::path::PathBuf;

use clap::ValueHint;

use crate::options::ConnectionOptions;
use crate::portable::repository::Channel;
use crate::portable::ver;

use edgedb_cli_derive::IntoArgs;


#[derive(clap::Args, Clone, Debug)]
#[command(version = "help_expand")]
#[command(disable_version_flag=true)]
pub struct Migration {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    #[command(subcommand)]
    pub subcommand: MigrationCmd,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum MigrationCmd {
    /// Apply migration from latest migration script.
    Apply(Box<Migrate>),
    /// Create migration script inside /migrations.
    Create(CreateMigration),
    /// Show current migration status.
    Status(ShowStatus),
    /// Show all migration versions.
    Log(MigrationLog),
    /// Edit migration file.
    ///
    /// Invokes $EDITOR on the last migration file, and then fixes
    /// migration id after editor exits. Defaults to vi (Notepad
    /// in Windows). Usually should be used for migrations that have
    /// not been applied yet.
    Edit(MigrationEdit),
    /// Check if current schema is compatible with new EdgeDB version.
    UpgradeCheck(UpgradeCheck),
    /// Extract migration history from the database and write it to
    /// <schema-dir>/migrations. Useful when a direct DDL command has
    /// been used to change the schema and now `edgedb migrate` will not
    /// comply because the database migration history is ahead of the
    /// migration history inside <schema-dir>/migrations.
    Extract(ExtractMigrations),
    /// Upgrades the format of migration files.
    UpgradeFormat(MigrationUpgradeFormat)
}

#[derive(clap::Args, IntoArgs, Clone, Debug)]
pub struct MigrationConfig {
    /// Project schema directory.  The default is `dbschema/`,
    /// which can be changed by setting `project.schema-dir`
    /// in `edgedb.toml`.
    #[arg(long, value_hint=ValueHint::DirPath)]
    pub schema_dir: Option<PathBuf>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct CreateMigration {
    #[command(flatten)]
    pub cfg: MigrationConfig,
    /// Squash all schema migrations into one and optionally provide a fixup migration.
    ///
    /// Note: this discards data migrations.
    #[arg(long)]
    pub squash: bool,
    /// Do not ask questions. By default works only if "safe" changes are
    /// to be done (those for which EdgeDB has a high degree of confidence).
    /// This safe default can be overridden with `--allow-unsafe`.
    #[arg(long)]
    pub non_interactive: bool,
    /// Apply the most probable unsafe changes in case there are ones. This
    /// is only useful in non-interactive mode.
    #[arg(long)]
    pub allow_unsafe: bool,
    /// Create a new migration even if there are no changes (use this for
    /// data-only migrations).
    #[arg(long)]
    pub allow_empty: bool,
    /// Print queries executed.
    #[arg(long, hide=true)]
    pub debug_print_queries: bool,
    /// Show error details.
    #[arg(long, hide=true)]
    pub debug_print_err: bool,
}

#[derive(clap::Args, Clone, Debug)]
pub struct Migrate {
    #[command(flatten)]
    pub conn: Option<ConnectionOptions>,

    #[command(flatten)]
    pub cfg: MigrationConfig,
    /// Do not print messages, only indicate success by exit status
    #[arg(long)]
    pub quiet: bool,

    /// Upgrade to a specified revision.
    ///
    /// A unique revision prefix can be specified instead of a full
    /// revision name.
    ///
    /// If this revision is applied, the command is a no-op. The command
    /// ensures that the revision is present, but additional applied revisions
    /// are not considered an error.
    #[arg(long, conflicts_with="dev_mode")]
    pub to_revision: Option<String>,

    /// Dev mode is used to temporarily apply schema on top of those found in
    /// the migration history. Usually used for testing purposes, as well as
    /// `edgedb watch` which creates a dev mode migration script each time
    /// a file is saved by a user.
    /// 
    /// Current dev mode migrations can be seen with the following query:
    /// 
    /// `select schema::Migration {*} filter .generated_by = schema::MigrationGeneratedBy.DevMode;`
    ///
    /// `edgedb migration create` followed by `edgedb migrate --dev-mode` will
    /// then finalize a migration by turning existing dev mode migrations into
    /// a regular `.edgeql` file, after which the above query will return nothing.
    #[arg(long)]
    pub dev_mode: bool,

    /// Runs the migration(s) in a single transaction.
    #[arg(long="single-transaction")]
    pub single_transaction: bool,
}

#[derive(clap::Args, Clone, Debug)]
pub struct ShowStatus {
    #[command(flatten)]
    pub cfg: MigrationConfig,

    /// Do not print any messages, only indicate success by exit status.
    #[arg(long)]
    pub quiet: bool,
}

#[derive(clap::Args, Clone, Debug)]
pub struct MigrationLog {
    #[command(flatten)]
    pub cfg: MigrationConfig,

    /// Print revisions from the filesystem.
    /// (Database connection not required.)
    #[arg(long)]
    pub from_fs: bool,

    /// Print revisions from the database.
    /// (No filesystem schema is required.)
    #[arg(long)]
    pub from_db: bool,

    /// Sort migrations starting from newer to older, instead
    /// of the default older to newer.
    #[arg(long)]
    pub newest_first: bool,

    /// Show maximum N revisions (default: no limit).
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct MigrationEdit {
    #[command(flatten)]
    pub cfg: MigrationConfig,

    /// Do not check migration using the database connection.
    #[arg(long)]
    pub no_check: bool,
    /// Fix migration id non-interactively, and do not run editor.
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(clap::Args, IntoArgs, Clone, Debug)]
pub struct UpgradeCheck {
    #[command(flatten)]
    pub cfg: MigrationConfig,

    /// Check upgrade to a specified version.
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_testing", "to_nightly", "to_channel",
    ])]
    pub to_version: Option<ver::Filter>,

    /// Check upgrade to latest nightly version.
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_testing", "to_channel",
    ])]
    pub to_nightly: bool,

    /// Check upgrade to latest testing version.
    #[arg(long)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_nightly", "to_channel",
    ])]
    pub to_testing: bool,

    /// Check upgrade to latest version in the channel.
    #[arg(long, value_enum)]
    #[arg(conflicts_with_all=&[
        "to_version", "to_nightly", "to_testing",
    ])]
    pub to_channel: Option<Channel>,

    /// Monitor schema changes and check again on change.
    #[arg(long)]
    pub watch: bool,

    #[arg(hide=true)]
    pub run_server_with_status: Option<PathBuf>,
}

#[derive(clap::Args, IntoArgs, Clone, Debug)]
pub struct ExtractMigrations {
    #[command(flatten)]
    pub cfg: MigrationConfig,
    /// Don't ask questions, only add missing files, abort if mismatching.
    #[arg(long)]
    pub non_interactive: bool,
    /// Force overwrite existing migration files.
    #[arg(long)]
    pub force: bool,
}

#[derive(clap::Args, IntoArgs, Clone, Debug)]
pub struct MigrationUpgradeFormat {
    #[command(flatten)]
    pub cfg: MigrationConfig,
}
