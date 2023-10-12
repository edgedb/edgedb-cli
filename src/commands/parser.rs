use std::path::PathBuf;

use clap::{ValueHint};
use edgedb_cli_derive::EdbClap;

use crate::repl::{self, VectorLimit};
use crate::options::ConnectionOptions;
use crate::migrations::options::{Migration, Migrate};


#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum Common {
    /// Create database backup
    Dump(Dump),
    /// Restore database from backup file
    Restore(Restore),
    /// Modify database configuration
    Configure(Configure),

    /// Migration management subcommands
    #[edb(expand_help)]
    Migration(Migration),
    /// Apply migration (alias for `edgedb migration apply`)
    Migrate(Migrate),

    /// Database commands
    #[edb(expand_help)]
    Database(Database),
    /// Describe database schema or object
    #[edb(expand_help)]
    Describe(Describe),

    /// List name and related info of database objects (types, scalars, modules, etc.)
    List(List),
    /// Analyze performance of query in quotes (e.g. `"select 9;"`)
    Analyze(Analyze),
    /// Show PostgreSQL address. Works on dev-mode database only.
    #[edb(hide=true)]
    Pgaddr,
    /// Run psql shell. Works on dev-mode database only.
    #[edb(hide=true)]
    Psql,
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
    /// Describe current database schema
    Schema(DescribeSchema),
}

#[derive(EdbClap, Clone, Debug)]
pub struct List {
    #[clap(subcommand)]
    pub subcommand: ListCmd,
}

#[derive(EdbClap, Clone, Debug)]
pub struct Analyze {
    /// Query to analyze performance of
    pub query: Option<String>,

    /// Write analysis into specified JSON file instead of formatting
    #[clap(long)]
    pub debug_output_file: Option<PathBuf>,

    /// Read JSON file instead of executing a query
    #[clap(long, conflicts_with="query")]
    pub read_json: Option<PathBuf>,

    /// Show detailed output of analyze command
    #[clap(long)]
    pub expand: bool,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum ListCmd {
    /// Display list of aliases defined in the schema
    Aliases(ListAliases),
    /// Display list of casts defined in the schema
    Casts(ListCasts),
    /// Display list of databases for an EdgeDB instance
    Databases,
    /// Display list of indexes defined in the schema
    Indexes(ListIndexes),
    /// Display list of modules defined in the schema
    Modules(ListModules),
    /// Display list of roles for an EdgeDB instance
    Roles(ListRoles),
    /// Display list of scalar types defined in the schema
    Scalars(ListTypes),
    /// Display list of object types defined in the schema
    Types(ListTypes),
}


#[derive(EdbClap, Clone, Debug)]
pub struct Database {
    #[clap(subcommand)]
    pub subcommand: DatabaseCmd,
}

#[derive(EdbClap, Clone, Debug)]
#[edb(inherit(ConnectionOptions))]
pub enum DatabaseCmd {
    /// Create a new database
    Create(CreateDatabase),
    /// Delete database along with its data
    Drop(DropDatabase),
    /// Preserve database while deleting its data
    Wipe(WipeDatabase),
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
    Expand,
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
    /// Show base state (before transaction) instead of current transaction
    /// state
    ///
    /// Has no effect if currently not in a transaction
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
    /// Maximum number of items to display per query (default 100). Specify 0 to disable.
    Limit(Limit),
    /// Set maximum number of elements to display for ext::pgvector::vector type.
    ///
    /// Defaults to `auto` which displays whatever fits a single line, but no less
    /// than 3. Can be set to `unlimited` or a fixed number.
    VectorDisplayLength(VectorLimitValue),
    /// Set output format
    OutputFormat(OutputFormat),
    /// Display typenames in default output mode
    DisplayTypenames(SettingBool),
    /// Disable escaping newlines in quoted strings
    ExpandStrings(SettingBool),
    /// Set number of entries retained in history
    HistorySize(SettingUsize),
    /// Print statistics on each query
    PrintStats(PrintStats),
    /// Set idle transaction timeout in Duration format.
    /// Default is 5 minutes; specify 0 to disable.
    IdleTransactionTimeout(IdleTransactionTimeout),
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct InputMode {
    #[clap(name="mode", value_parser=["vi", "emacs"])]
    pub value: Option<repl::InputMode>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct SettingBool {
    #[clap(value_parser=["on", "off", "true", "false"])]
    pub value: Option<String>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct Limit {
    #[clap(name="limit")]
    pub value: Option<usize>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct VectorLimitValue {
    #[clap(name="limit")]
    pub value: Option<VectorLimit>,
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
pub struct Edit {
    #[clap(trailing_var_arg=true, allow_hyphen_values=true)]
    pub entry: Option<isize>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct OutputFormat {
    #[clap(name="mode", value_parser=
        ["default", "json-pretty", "json", "json-lines", "tab-separated"]
    )]
    pub value: Option<repl::OutputFormat>,
}

#[derive(EdbClap, Clone, Debug, Default)]
pub struct PrintStats {
    #[clap(value_parser=
        ["off", "query", "detailed"]
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
pub struct DropDatabase {
    pub database_name: String,
    /// Drop database without confirming
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct WipeDatabase {
    /// Drop database without confirming
    #[clap(long)]
    pub non_interactive: bool,
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
    /// Use dash `-` to write to stdout (latter does not work in `--all` mode)
    #[clap(value_hint=ValueHint::AnyPath)]
    pub path: PathBuf,
    /// Dump all databases and server configuration. `path` is a directory
    /// in this case
    #[clap(long)]
    pub all: bool,

    /// Include secret configuration variables in the dump
    #[clap(long)]
    pub include_secrets: bool,

    /// Choose dump format. For normal dumps this parameter should be omitted.
    /// For `--all`, only `--format=dir` is required.
    #[clap(long, value_parser=["dir"])]
    pub format: Option<DumpFormat>,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(override_usage(
    "edgedb restore [OPTIONS] <path>\n    \
     edgedb restore -d <database-name> <path>"
))]
pub struct Restore {
    /// Path to file (or directory in case of `--all`) to read dump from.
    /// Use dash `-` to read from stdin
    #[clap(value_hint=ValueHint::AnyPath)]
    pub path: PathBuf,

    /// Restore all databases and server configuration. `path` is a
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
    /// If the list is empty, the server will not listen on any IP interface
    /// whatsoever, in which case only Unix-domain sockets can be used to
    /// connect to it.
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

    /// The maximum amount of memory to be used by maintenance operations.
    ///
    /// Some of the operations that use this option are: vacuuming, link, index
    /// or constraint creation. A value without units is assumed to be
    /// kilobytes. Defaults to 64 megabytes (64MB).
    ///
    /// Corresponds to the PostgreSQL maintenance_work_mem configuration
    /// parameter.
    MaintenanceWorkMem(ConfigStr),

    /// Sets the planner’s assumption about the effective size of the disk
    /// cache available to a single query.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name.
    EffectiveCacheSize(ConfigStr),

    /// Sets the default data statistics target for the planner.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name.
    DefaultStatisticsTarget(ConfigStr),

    /// Sets the number of concurrent disk I/O operations that PostgreSQL
    /// expects can be executed simultaneously.
    ///
    /// Corresponds to the PostgreSQL configuration parameter of the same name.
    EffectiveIoConcurrency(ConfigStr),

    /// How long client connections can stay inactive before being closed by
    /// the server. Defaults to `60 seconds`; set to `0s` to disable.
    SessionIdleTimeout(ConfigStr),

    /// How long client connections can stay inactive while in a transaction.
    /// Defaults to 10 seconds; set to `0s` to disable.
    SessionIdleTransactionTimeout(ConfigStr),

    /// How long an individual query can run before being aborted. A value of
    /// `0s` disables the mechanism; it is disabled by default.
    QueryExecutionTimeout(ConfigStr),

    /// Defines whether to allow DDL commands outside of migrations.
    ///
    /// May be set to:
    /// * `AlwaysAllow`
    /// * `NeverAllow`
    AllowBareDdl(ConfigStr),

    /// Apply access policies
    ///
    /// User-specified access policies are not applied when set to `false`,
    /// allowing any queries to be executed.
    ApplyAccessPolicies(ConfigBool),

    /// Allow setting user-specified object identifiers.
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
    /// Reset shared_buffers PostgreSQL configuration parameter to default value
    SharedBuffers,
    /// Reset work_mem PostgreSQL configuration parameter to default value
    QueryWorkMem,
    /// Reset PostgreSQL configuration parameter of the same name
    MaintenanceWorkMem,
    /// Reset PostgreSQL configuration parameter of the same name
    EffectiveCacheSize,
    /// Reset PostgreSQL configuration parameter of the same name
    DefaultStatisticsTarget,
    /// Reset PostgreSQL configuration parameter of the same name
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
    /// Priority of the authentication rule. The lower the number, the
    /// higher the priority.
    #[clap(long)]
    pub priority: i64,

    /// The name(s) of the database role(s) this rule applies to. Will apply
    /// to all roles if set to '*'
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
