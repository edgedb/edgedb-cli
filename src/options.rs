use std::env;
use std::time::Duration;
use std::process::exit;

use atty;
use structopt::StructOpt;
use structopt::clap::AppSettings;
use whoami;

use crate::repl::OutputMode;
use crate::commands::parser::Common;


#[derive(StructOpt, Debug)]
struct TmpOptions {
    /// Host of the EdgeDB instance
    #[structopt(short="H", long)]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[structopt(short="P", long)]
    pub port: Option<u16>,

    /// User name of the EdgeDB user
    #[structopt(short="u", long)]
    pub user: Option<String>,

    /// Database name to connect to
    #[structopt(short="d", long)]
    pub database: Option<String>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default
    #[structopt(long)]
    pub admin: bool,

    /// Ask for password on the terminal (TTY)
    #[structopt(long)]
    pub password: bool,

    /// Don't ask for password
    #[structopt(long)]
    pub no_password: bool,

    /// Read the password from stdin rather than TTY (useful for scripts)
    #[structopt(long)]
    pub password_from_stdin: bool,

    /// In case EdgeDB connection can't be established, retry up to N seconds.
    #[structopt(long, name="N",
                parse(try_from_str=humantime::parse_duration))]
    pub wait_until_available: Option<Duration>,

    #[structopt(long)]
    pub debug_print_frames: bool,
    #[structopt(long)]
    pub debug_print_descriptors: bool,
    #[structopt(long)]
    pub debug_print_codecs: bool,

    /// Tab-separated output of the queries
    #[structopt(short="t", long, overrides_with="json")]
    pub tab_separated: bool,

    /// JSON output for the queries (single JSON list per query)
    #[structopt(short="j", long, overrides_with="tab_separated")]
    pub json: bool,

    /// Execute a query instead of starting REPL (alias to `edgedb query`)
    #[structopt(short="c")]
    pub query: Option<String>,

    #[structopt(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Password {
    NoPassword,
    FromTerminal,
    Password(String),
}

#[derive(StructOpt, Clone, Debug)]
pub enum Command {
    AlterRole(RoleParams),
    CreateSuperuserRole(RoleParams),
    DropRole(RoleName),
    Query(Query),
    #[structopt(flatten)]
    Common(Common),
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(setting=AppSettings::DisableVersion)]
pub struct Query {
    pub queries: Vec<String>,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(setting=AppSettings::DisableVersion)]
pub struct RoleParams {
    /// Role name
    pub role: String,
    /// Set the password for role (read separately from the terminal)
    #[structopt(long="password")]
    pub password: bool,
    /// Set the password for role, read from the stdin
    #[structopt(long)]
    pub password_from_stdin: bool,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(setting=AppSettings::DisableVersion)]
pub struct RoleName {
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: String,
    pub admin: bool,
    pub password: Password,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_mode: OutputMode,
    pub wait_until_available: Option<Duration>,
}

impl Options {
    pub fn from_args_and_env() -> Options {
        let tmp = TmpOptions::from_args();
        let admin = tmp.admin;
        let user = tmp.user
            .or_else(|| env::var("EDGEDB_USER").ok())
            .unwrap_or_else(|| if admin  {
                String::from("edgedb")
            } else {
                whoami::username()
            });
        let host = tmp.host
            .or_else(|| env::var("EDGEDB_HOST").ok())
            .unwrap_or_else(|| String::from("localhost"));
        let port = tmp.port
            .or_else(|| env::var("EDGEDB_PORT").ok()
                        .and_then(|x| x.parse().ok()))
            .unwrap_or_else(|| 5656);
        let database = tmp.database
            .or_else(|| env::var("EDGEDB_DATABASE").ok())
            .unwrap_or_else(|| if admin  {
                String::from("edgedb")
            } else {
                user.clone()
            });

        // TODO(pc) add option to force interactive mode not on a tty (tests)
        let interactive = tmp.query.is_none()
            && tmp.subcommand.is_none()
            && atty::is(atty::Stream::Stdin);
        let password = if tmp.password_from_stdin {
            let password = rpassword::read_password()
                .expect("password can be read");
            Password::Password(password)
        } else if tmp.no_password {
            Password::NoPassword
        } else {
            Password::FromTerminal
        };

        let subcommand = if let Some(query) = tmp.query {
            if tmp.subcommand.is_some() {
                eprintln!("Option `-c` conflicts with specifying subcommand");
                exit(1);
            } else {
                Some(Command::Query(Query {
                    queries: vec![query],
                }))
            }
        } else {
            tmp.subcommand
        };

        return Options {
            host, port, user, database, interactive,
            admin: tmp.admin,
            subcommand,
            password,
            debug_print_frames: tmp.debug_print_frames,
            debug_print_descriptors: tmp.debug_print_descriptors,
            debug_print_codecs: tmp.debug_print_codecs,
            wait_until_available: tmp.wait_until_available,
            output_mode: if tmp.tab_separated {
                OutputMode::TabSeparated
            } else if tmp.json {
                OutputMode::Json
            } else if interactive {
                OutputMode::Default
            } else {
                OutputMode::JsonElements
            },
        }
    }
}
