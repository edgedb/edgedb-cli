use clap::{Clap, AppSettings, ArgSettings, ValueHint};
use std::path::PathBuf;

use crate::repl;


#[derive(Clap, Clone, Debug)]
pub enum Common {
    /// Create a new database
    CreateDatabase(CreateDatabase),
    /// Display list of databases in the server instance
    ListDatabases,
    /// List ports exposed by EdgeDB. Works on EdgeDB <= 1-alpha7
    #[clap(setting=AppSettings::Hidden)]
    ListPorts,
    /// Show postgres address. Works on dev-mode database only.
    #[clap(setting=AppSettings::Hidden)]
    Pgaddr,
    /// Run psql shell. Works on dev-mode database only.
    #[clap(setting=AppSettings::Hidden)]
    Psql,
    /// Display list of aliases defined in the schema
    ListAliases(ListAliases),
    /// Display list of casts defined in the schema
    ListCasts(ListCasts),
    /// Display list of indexes defined in the schema
    ListIndexes(ListIndexes),
    /// Display list of scalar types defined in the schema
    ListScalarTypes(ListTypes),
    /// Display list of object types defined in the schema
    ListObjectTypes(ListTypes),
    /// Display list of roles in the server instance
    ListRoles(ListRoles),
    /// Display list of modules defined in the schema
    ListModules(ListModules),
    /// Modify database configuration
    Configure(Configure),
    /// Describe a named database object
    Describe(Describe),
    /// Describe schema of the current database
    DescribeSchema(DescribeSchema),
    /// Create a database backup
    Dump(Dump),
    /// Restore a database backup from file
    Restore(Restore),
    /// Create a migration script
    CreateMigration(CreateMigration),
    /// Bring current database to the latest or a specified revision
    Migrate(Migrate),
    /// Show current migration state
    ShowStatus(ShowStatus),
    /// Show all migration versions
    MigrationLog(MigrationLog),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::NoBinaryName)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Backslash {
    #[clap(subcommand)]
    pub command: BackslashCmd,
}

#[derive(Clap, Clone, Debug)]
pub enum BackslashCmd {
    #[clap(flatten)]
    Common(Common),
    Help,
    LastError,
    History,
    Connect(Connect),
    Edit(Edit),
    Set(SetCommand),
    Exit,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct SetCommand {
    #[clap(subcommand)]
    pub setting: Option<Setting>,
}

#[derive(Clap, Clone, Debug)]
pub enum Setting {
    /// Set input mode. One of: vi, emacs
    InputMode(InputMode),
    /// Print implicit properties of objects: id, type id
    ImplicitProperties(SettingBool),
    /// Print all errors with maximum verbosity
    VerboseErrors(SettingBool),
    /// Set implicit LIMIT. Defaults to 100, specify 0 to disable.
    Limit(Limit),
    /// Set output mode. One of: json, json-elements, default, tab-separated
    OutputMode(OutputMode),
    /// Stop escaping newlines in quoted strings
    ExpandStrings(SettingBool),
    /// Set number of entries retained in history
    HistorySize(SettingUsize),
    /// Print statistics on each query
    PrintStats(PrintStats),
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct InputMode {
    #[clap(possible_values=&["vi", "emacs"][..])]
    pub mode: Option<repl::InputMode>,
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct SettingBool {
    #[clap(possible_values=&["on", "off"][..])]
    pub value: Option<String>,
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Limit {
    pub limit: Option<usize>,
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct SettingUsize {
    pub value: Option<usize>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
#[clap(setting=AppSettings::TrailingVarArg)]
#[clap(setting=AppSettings::AllowLeadingHyphen)]
pub struct Edit {
    pub entry: Option<isize>,
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct OutputMode {
    #[clap(possible_values=
        &["json", "json-elements", "default", "tab-separated"][..]
    )]
    pub mode: Option<repl::OutputMode>,
}

#[derive(Clap, Clone, Debug, Default)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct PrintStats {
    #[clap(possible_values=
        &["off", "query", "detailed"][..]
    )]
    pub value: Option<repl::PrintStats>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Connect {
    pub database_name: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct CreateDatabase {
    pub database_name: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListAliases {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListCasts {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListIndexes {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListTypes {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
    #[clap(long, short='s')]
    pub system: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListRoles {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListModules {
    pub pattern: Option<String>,
    #[clap(long, short='I')]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Describe {
    pub name: String,
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct DescribeSchema {
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpFormat {
    Dir,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Restore {
    /// Path to file (or directory in case of `--all) to read dump from.
    /// Use dash `-` to read from stdin
    #[clap(value_hint=ValueHint::AnyPath)]
    pub path: PathBuf,

    /// Restore all databases and the server configuratoin. `path` is a
    /// directory in this case
    #[clap(long)]
    pub all: bool,

    /// Allow restoring the database dump into a non-empty database
    #[clap(long)]
    pub allow_non_empty: bool,

    /// Verbose output
    #[clap(long, short='v')]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Configure {
    #[clap(subcommand)]
    pub command: ConfigureCommand,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub enum ConfigureCommand {
    /// Insert another configuration entry to the list setting
    Insert(ConfigureInsert),
    /// Reset configuration entry (empty the list for list settings)
    Reset(ConfigureReset),
    /// Set scalar configuration value
    Set(ConfigureSet),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ConfigureInsert {
    #[clap(subcommand)]
    pub parameter: ListParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ConfigureReset {
    #[clap(subcommand)]
    pub parameter: ConfigParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ConfigureSet {
    #[clap(subcommand)]
    pub parameter: ValueParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub enum ListParameter {

    /// Insert a client authentication rule
    #[clap(name="Auth")]
    Auth(AuthParameter),

    /// Insert an application port with the specicified protocol
    #[clap(name="Port")]
    Port(PortParameter),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
#[clap(rename_all="snake_case")]
pub enum ConfigParameter {
    /// Reset listen addresses to 127.0.0.1
    ListenAddresses,
    /// Reset port to 5656
    ListenPort,
    /// Remove all the application ports
    #[clap(name="Port")]
    Port,
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
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListenAddresses {
    pub address: Vec<String>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ListenPort {
    pub port: u16,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ConfigStr {
    pub value: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct PortParameter {

    /// The TCP/IP address(es) for the application port.
    #[clap(long="address")]
    pub addresses: Vec<String>,

    /// The TCP port for the application port.
    #[clap(long)]
    pub port: u16,

    /// The protocol for the application port. Valid values are:
    /// 'graphql+http' and 'edgeql+http'.
    #[clap(long)]
    pub protocol: String,

    /// The name of the database the application port is attached to.
    #[clap(long)]
    pub database: String,

    /// The name of the database role the application port is attached to.
    #[clap(long)]
    pub user: String,

    /// The maximum number of backend connections available for this
    /// application port.
    #[clap(long)]
    pub concurrency: i64,
}

#[derive(Clap, Clone, Debug)]
pub struct MigrationConfig {
    /// Directory where *.edgeql files are located
    #[clap(long, default_value="./dbschema", value_hint=ValueHint::DirPath)]
    pub schema_dir: PathBuf,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct CreateMigration {
    #[clap(flatten)]
    pub cfg: MigrationConfig,
    /// Do not ask questions. By default works only if "safe" changes are
    /// to be done. Unless `--allow-unsafe` is also specified.
    #[clap(long)]
    pub non_interactive: bool,
    /// Apply the most propbable unsafe changes in case there are ones. This
    /// is only useful in non-interactive mode
    #[clap(long)]
    pub allow_unsafe: bool,
    /// Create a new migration even if there are no changes (use this for
    /// data-only migrations)
    #[clap(long)]
    pub allow_empty: bool,
    /// Print queries executed
    #[clap(long, setting=ArgSettings::Hidden)]
    pub debug_print_queries: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct ShowStatus {
    #[clap(flatten)]
    pub cfg: MigrationConfig,

    /// Do not print any messages, only indicate success by exit status
    #[clap(long)]
    pub quiet: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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

    /// Sort migrations starting from never to older,
    /// by default older revisions go first
    #[clap(long)]
    pub newest_first: bool,

    /// Show maximum N revisions (default is unlimited)
    #[clap(long)]
    pub limit: Option<usize>,
}

impl Setting {
    pub fn name(&self) -> &'static str {
        use Setting::*;
        match self {
            InputMode(_) => "input-mode",
            ImplicitProperties(_) => "implicit-properties",
            VerboseErrors(_) => "verbose-errors",
            Limit(_) => "limit",
            HistorySize(_) => "history-size",
            OutputMode(_) => "output-mode",
            ExpandStrings(_) => "expand-strings",
            PrintStats(_) => "print-stats",
        }
    }
    pub fn is_show(&self) -> bool {
        use Setting::*;

        match self {
            InputMode(a) => a.mode.is_none(),
            ImplicitProperties(a) => a.value.is_none(),
            VerboseErrors(a) => a.value.is_none(),
            Limit(a) => a.limit.is_none(),
            HistorySize(a) => a.value.is_none(),
            OutputMode(a) => a.mode.is_none(),
            ExpandStrings(a) => a.value.is_none(),
            PrintStats(a) => a.value.is_none(),
        }
    }
}

impl SettingBool {
    pub fn unwrap_value(&self) -> bool {
        match self.value.as_deref() {
            Some("on") => true,
            Some("off") => false,
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
