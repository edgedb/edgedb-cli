use std::env;
use std::time::Duration;
use std::process::exit;

use atty;
use clap::{Clap, AppSettings};
use whoami;

use crate::repl::OutputMode;
use crate::commands::parser::Common;
use crate::server;


#[derive(Clap, Debug)]
#[clap(version=clap::crate_version!())]
struct TmpOptions {
    /// Host of the EdgeDB instance
    #[clap(short="H", long)]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[clap(short="P", long)]
    pub port: Option<u16>,

    /// User name of the EdgeDB user
    #[clap(short="u", long)]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short="d", long)]
    pub database: Option<String>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default
    #[clap(long)]
    pub admin: bool,

    /// Ask for password on the terminal (TTY)
    #[clap(long)]
    pub password: bool,

    /// Don't ask for password
    #[clap(long)]
    pub no_password: bool,

    /// Read the password from stdin rather than TTY (useful for scripts)
    #[clap(long)]
    pub password_from_stdin: bool,

    /// In case EdgeDB connection can't be established, retry up to N seconds.
    #[clap(long, name="N",
                parse(try_from_str=humantime::parse_duration))]
    pub wait_until_available: Option<Duration>,

    #[clap(long)]
    pub debug_print_frames: bool,
    #[clap(long)]
    pub debug_print_descriptors: bool,
    #[clap(long)]
    pub debug_print_codecs: bool,

    /// Tab-separated output of the queries
    #[clap(short="t", long, overrides_with="json")]
    pub tab_separated: bool,

    /// JSON output for the queries (single JSON list per query)
    #[clap(short="j", long, overrides_with="tab_separated")]
    pub json: bool,

    /// Execute a query instead of starting REPL (alias to `edgedb query`)
    #[clap(short="c")]
    pub query: Option<String>,

    #[clap(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Password {
    NoPassword,
    FromTerminal,
    Password(String),
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    AlterRole(RoleParams),
    CreateSuperuserRole(RoleParams),
    DropRole(RoleName),
    Query(Query),
    Server(server::options::ServerCommand),
    #[clap(flatten)]
    Common(Common),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct Query {
    pub queries: Vec<String>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
pub struct RoleParams {
    /// Role name
    pub role: String,
    /// Set the password for role (read separately from the terminal)
    #[clap(long="password")]
    pub password: bool,
    /// Set the password for role, read from the stdin
    #[clap(long)]
    pub password_from_stdin: bool,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersion)]
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
        let tmp = TmpOptions::parse();
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
        } else if tmp.password {
            Password::FromTerminal
        } else {
            match env::var("EDGEDB_PASSWORD") {
                Ok(p) => Password::Password(p),
                Err(_) => Password::NoPassword
            }
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
