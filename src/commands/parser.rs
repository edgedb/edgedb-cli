use clap::{Clap, AppSettings};
use std::path::PathBuf;

use crate::repl::OutputMode;


#[derive(Clap, Clone, Debug)]
pub enum Common {
    CreateDatabase(CreateDatabase),
    ListDatabases,
    ListPorts,
    Pgaddr,
    Psql,
    ListAliases(ListAliases),
    ListCasts(ListCasts),
    ListIndexes(ListIndexes),
    ListScalarTypes(ListTypes),
    ListObjectTypes(ListTypes),
    ListRoles(ListRoles),
    ListModules(ListModules),
    /// Modify database configuration
    Configure(Configure),
    Describe(Describe),
    /// Create a database backup
    Dump(Dump),
    /// Restore a database backup from file
    Restore(Restore),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::NoBinaryName)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Backslash {
    #[clap(subcommand)]
    pub command: BackslashCmd,
}

#[derive(Clap, Clone, Debug)]
pub enum BackslashCmd {
    #[clap(flatten)]
    Common(Common),
    Help,
    ViMode,
    EmacsMode,
    ImplicitProperties,
    NoImplicitProperties,
    IntrospectTypes,
    NoIntrospectTypes,
    LastError,
    VerboseErrors,
    NoVerboseErrors,
    History,
    Limit(Limit),
    Output(Output),
    Connect(Connect),
    Edit(Edit),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Limit {
    pub limit: Option<usize>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Edit {
    pub entry: Option<isize>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Output {
    pub mode: Option<OutputMode>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Connect {
    pub database_name: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct CreateDatabase {
    pub database_name: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListAliases {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
    #[clap(long, short="S")]
    pub system: bool,
    #[clap(long, short="v")]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListCasts {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListIndexes {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
    #[clap(long, short="S")]
    pub system: bool,
    #[clap(long, short="v")]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListTypes {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
    #[clap(long, short="S")]
    pub system: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListRoles {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListModules {
    pub pattern: Option<String>,
    #[clap(long, short="I")]
    pub case_sensitive: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Describe {
    pub name: String,
    #[clap(long, short="v")]
    pub verbose: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Dump {
    pub file: PathBuf,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Restore {
    pub file: PathBuf,

    /// Allow restoring the database dump into a non-empty database
    #[clap(long)]
    pub allow_non_empty: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Configure {
    #[clap(subcommand)]
    pub command: ConfigureCommand,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub enum ConfigureCommand {
    /// Insert another configuration entry to the list setting
    Insert(ConfigureInsert),
    /// Reset configuration entry (empty the list for list settings)
    Reset(ConfigureReset),
    /// Set scalar configuration value
    Set(ConfigureSet),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ConfigureInsert {
    #[clap(subcommand)]
    pub parameter: ListParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ConfigureReset {
    #[clap(subcommand)]
    pub parameter: ConfigParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ConfigureSet {
    #[clap(subcommand)]
    pub parameter: ValueParameter,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub enum ListParameter {

    /// Insert a client authentication rule
    #[clap(name="Auth")]
    Auth(AuthParameter),

    /// Insert an application port with the specicified protocol
    #[clap(name="Port")]
    Port(PortParameter),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
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
#[clap(setting=AppSettings::DisableVersion)]
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
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListenAddresses {
    pub address: Vec<String>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ListenPort {
    pub port: u16,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct ConfigStr {
    pub value: String,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
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
#[clap(setting=AppSettings::DisableVersion)]
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
