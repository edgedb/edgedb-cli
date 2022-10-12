use std::path::PathBuf;

use clap::{ValueHint};
use edgedb_cli_derive::EdbClap;

use crate::repl;
use crate::options::ConnectionOptions;


#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum Common {
    /// Create a database backup
    Dump(Dump),
    /// Restore a database backup from file
    Restore(Restore),
    /// Modify database configuration
    Configure(Configure),

    /// Migration management subcommands
    #[edb(expand_help)]
    Migration(Migration),
    /// An alias for `edgedb migration apply`
    Migrate(Migrate),

    /// Database commands
    #[edb(expand_help)]
    Database(Database),
    /// Describe database schema or an object
    #[edb(expand_help)]
    Describe(Describe),

    /// List matching database objects by name and type
    List(List),
    /// Show postgres address. Works on dev-mode database only.
    #[edb(hide=true)]
    Pgaddr,
    /// Run psql shell. Works on dev-mode database only.
    #[edb(hide=true)]
    Psql,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Migration {
    #[clap(subcommand)]
    pub subcommand: MigrationCmd,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Describe {
    #[clap(subcommand)]
    pub subcommand: DescribeCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum DescribeCmd {
    /// Describe a database object
    Object(DescribeObject),
    /// Describe schema of the current database
    Schema(DescribeSchema),
}

#[derive(EdbClap, Clone, Debug)]
pub struct List {
    #[clap(subcommand)]
    pub subcommand: ListCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum ListCmd {
    /// Display list of aliases defined in the schema
    Aliases(ListAliases),
    /// Display list of casts defined in the schema
    Casts(ListCasts),
    /// Display list of databases in the EdgeDB instance
    Databases,
    /// Display list of indexes defined in the schema
    Indexes(ListIndexes),
    /// Display list of modules defined in the schema
    Modules(ListModules),
    /// Display list of roles in the EdgeDB instance
    Roles(ListRoles),
    /// Display list of scalar types defined in the schema
    Scalars(ListTypes),
    /// Display list of object types defined in the schema
    Types(ListTypes),
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
pub struct Database {
    #[clap(subcommand)]
    pub subcommand: DatabaseCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum DatabaseCmd {
    /// Create a new DB
    Create(CreateDatabase),
}

#[derive(EdbClap, Clone, Debug)]
#[clap(no_binary_name=true)]
pub struct Backslash {
    #[clap(subcommand)]
    pub command: BackslashCmd,
}

#[derive(EdbClap, Clone, Debug)]
pub enum BackslashCmd {
    #[clap(flatten)]
    Common(Common),
    Help,
    LastError,
    DebugState(StateParam),
    DebugStateDesc(StateParam),
    History,
    Connect(Connect),
    Edit(Edit),
    Set(SetCommand),
    Exit,
}

#[derive(EdbClap, Clone, Debug)]
pub struct StateParam {
    /// Show base state (before the transaction) instead of current transaction
    /// state
    ///
    /// Has no meaning if currently not in transaction
    #[clap(short='b')]
    pub base: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct SetCommand {
    #[clap(subcommand)]
    pub setting: Option<Setting>,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(setting_impl)]
pub enum Setting {
    /// Set input mode. One of: vi, emacs
    InputMode(InputMode),
    /// Print implicit properties of objects: id, type id
    ImplicitProperties(SettingBool),
    /// Print all errors with maximum verbosity
    VerboseErrors(SettingBool),
    /// Set implicit LIMIT. Defaults to 100, specify 0 to disable.
    Limit(Limit),
    /// Set output format.
    OutputFormat(OutputFormat),
    /// Display typenames in default output mode
    DisplayTypenames(SettingBool),
    /// Stop escaping newlines in quoted strings
    ExpandStrings(SettingBool),
    /// Set number of entries retained in history
    HistorySize(SettingUsize),
    /// Print statistics on each query
    PrintStats(PrintStats),
    /// Set idle transaction timeout in Duration format.
    /// Defaults to 5 minutes, specify 0 to disable.
    IdleTransactionTimeout(IdleTransactionTimeout),
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct InputMode {
    #[clap(name="mode", possible_values=&["vi", "emacs"][..])]
    pub value: Option<repl::InputMode>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct SettingBool {
    #[clap(possible_values=&["on", "off", "true", "false"][..])]
    pub value: Option<String>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct Limit {
    #[clap(name="limit")]
    pub value: Option<usize>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct IdleTransactionTimeout {
    #[clap(name="duration")]
    pub value: Option<String>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct SettingUsize {
    pub value: Option<usize>,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(trailing_var_arg=true, allow_hyphen_values=true)]
pub struct Edit {
    pub entry: Option<isize>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct OutputFormat {
    #[clap(name="mode", possible_values=
        &["default", "json-pretty", "json", "json-lines", "tab-separated"][..]
    )]
    pub value: Option<repl::OutputFormat>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct PrintStats {
    #[clap(possible_values=
        &["off", "query", "detailed"][..]
    )]
    pub value: Option<repl::PrintStats>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Connect {
    pub database_name: String,
}

#[derive(EdbClap, Clone, Debug)]
pub struct CreateDatabase {
    pub database_name: String,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListAliases {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListCasts {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListIndexes {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListTypes {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListRoles {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListModules {
    pub pattern: Option<String>,
    #[clap(long, short='c')]
    pub case_sensitive: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct DescribeObject {
    pub name: String,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct DescribeSchema {
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpFormat {
    Dir,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Dump {
    /// Path to file write dump to (or directory if `--all` is specified).
    /// Use dash `-` to write into stdout (latter does not work in `--all` mode)
    #[clap(value_hint=ValueHint::AnyPath)]
    pub path: PathBuf,
    /// Dump all databases and the server configuration. `path` is a directory
    /// in this case
    #[clap(long)]
    pub all: bool,

    /// Choose dump format. For normal dumps this parameter should be omitted.
    /// For `--all` only `--format=dir` is required.
    #[clap(long, possible_values=&["dir"][..])]
    pub format: Option<DumpFormat>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Restore {
    /// Path to file (or directory in case of `--all`) to read dump from.
    /// Use dash `-` to read from stdin
    #[clap(value_hint=ValueHint::AnyPath)]
    pub path: PathBuf,

    /// Restore all databases and the server configuratoin. `path` is a
    /// directory in this case
    #[clap(long)]
    pub all: bool,

    /// Verbose output
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Configure {
    #[clap(subcommand)]
    pub command: ConfigureCommand,
}

#[derive(EdbClap, Clone, Debug)]
pub enum ConfigureCommand {
    /// Insert another configuration entry to the list setting
    Insert(ConfigureInsert),
    /// Reset configuration entry (empty the list for list settings)
    Reset(ConfigureReset),
    /// Set scalar configuration value
    Set(ConfigureSet),
}

#[derive(EdbClap, Clone, Debug)]
pub struct ConfigureInsert {
    #[clap(subcommand)]
    pub parameter: ListParameter,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ConfigureReset {
    #[clap(subcommand)]
    pub parameter: ConfigParameter,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ConfigureSet {
    #[clap(subcommand)]
    pub parameter: ValueParameter,
}

#[derive(EdbClap, Clone, Debug)]
pub enum ListParameter {

    /// Insert a client authentication rule
    #[clap(name="Auth")]
    Auth(AuthParameter),
}

#[derive(EdbClap, Clone, Debug)]
#[clap(rename_all="snake_case")]
pub enum ValueParameter {
    /// Specifies the TCP/IP address(es) on which the server is to listen for
    /// connections from client applications.
    ///
    /// If the list is empty, the server does not listen on any IP interface
    /// at all, in which case only Unix-domain sockets can be used to connect
    /// to it.
    ListenAddresses(ListenAddresses),

    /// The TCP port the server listens on; 5656 by default. Note that the
    /// same port number is used for all IP addresses the server listens on.
    ListenPort(ListenPort),

    /// The amount of memory the database uses for shared memory buffers.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same
    /// name. Changing this value requires server restart.
    SharedBuffers(ConfigStr),

    /// The amount of memory used by internal query operations such as sorting.
    ///
    /// Corresponds to the PostgreSQL work_mem configuration parameter.
    QueryWorkMem(ConfigStr),

    /// Sets the planner’s assumption about the effective size of the disk
    /// cache that is available to a single query.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name
    EffectiveCacheSize(ConfigStr),

    /// Sets the default data statistics target for the planner.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name
    DefaultStatisticsTarget(ConfigStr),

    /// Sets the number of concurrent disk I/O operations that PostgreSQL
    /// expects can be executed simultaneously
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name
    EffectiveIoConcurrency(ConfigStr),

    /// How long client connections can stay inactive before being closed by
    /// the server. Defaults to `60 seconds`; set to `0s` to disable
    /// the mechanism.
    SessionIdleTimeout(ConfigStr),

    /// How long client connections can stay inactive while in a transaction.
    /// Defaults to `10 seconds`; set to `0s` to disable the
    /// mechanism.
    SessionIdleTransactionTimeout(ConfigStr),

    /// How long an individual query can run before being aborted. A value of
    /// `0s` disables the mechanism; it is disabled by default.
    QueryExecutionTimeout(ConfigStr),

    /// Defines whether DDL commands that aren't migrations are allowed
    ///
    /// May be set to:
    /// * `AlwaysAllow`
    /// * `NeverAllow`
    AllowBareDdl(ConfigStr),

    /// Apply access policies
    ///
    /// When set to `false` user-specified access policies are not applied, so
    /// any queries may be executed.
    ApplyAccessPolicies(ConfigBool),

    /// Allow setting user-specified object identifiers
    AllowUserSpecifiedId(ConfigBool),
}

#[derive(EdbClap, Clone, Debug)]
#[clap(rename_all="snake_case")]
pub enum ConfigParameter {
    /// Reset listen addresses to 127.0.0.1
    ListenAddresses,
    /// Reset port to 5656
    ListenPort,
    /// Clear authentication table (only admin socket can be used to connect)
    #[clap(name="Auth")]
    Auth,
    /// Reset shared_buffers postgres configuration parameter to default value
    SharedBuffers,
    /// Reset work_mem postgres configuration parameter to default value
    QueryWorkMem,
    /// Reset postgres configuration parameter of the same name
    EffectiveCacheSize,
    /// Reset postgres configuration parameter of the same name
    DefaultStatisticsTarget,
    /// Reset postgres configuration parameter of the same name
    EffectiveIoConcurrency,
    /// Reset session idle timeout
    SessionIdleTimeout,
    /// Reset session idle transaction timeout
    SessionIdleTransactionTimeout,
    /// Reset query execution timeout
    QueryExecutionTimeout,
    /// Reset allow_bare_ddl parameter to `AlwaysAllow`
    AllowBareDdl,
    /// Reset apply_access_policies parameter to `true`
    ApplyAccessPolicies,
    /// Reset allow_user_specified_id parameter to `false`
    AllowUserSpecifiedId,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListenAddresses {
    pub address: Vec<String>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ListenPort {
    pub port: u16,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ConfigStr {
    pub value: String,
}

#[derive(EdbClap, Clone, Debug)]
pub struct ConfigBool {
    pub value: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct AuthParameter {
    /// The priority of the authentication rule. The lower this number, the
    /// higher the priority.
    #[clap(long)]
    pub priority: i64,

    /// The name(s) of the database role(s) this rule applies to. If set to
    /// '*', then it applies to all roles.
    #[clap(long="user")]
    pub users: Vec<String>,

    /// The name of the authentication method type. Valid values are: Trust
    /// for no authentication and SCRAM for SCRAM-SHA-256 password
    /// authentication.
    #[clap(long)]
    pub method: String,

    /// An optional comment for the authentication rule.
    #[clap(long)]
    pub comment: Option<String>,
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
    #[clap(long)]
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

impl SettingBool {
    pub fn unwrap_value(&self) -> bool {
        match self.value.as_deref() {
            Some("on") => true,
            Some("off") => false,
            Some("true") => true,
            Some("false") => false,
            _ => unreachable!("validated by clap"),
        }
    }
}

impl std::str::FromStr for DumpFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<DumpFormat, anyhow::Error> {
        match s {
            "dir" => Ok(DumpFormat::Dir),
            _ => Err(anyhow::anyhow!("unsupported dump format {:?}", s)),
        }
    }
}
