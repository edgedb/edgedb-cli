use std::env;
use std::time::Duration;
use std::process::exit;

use atty;
use clap::{Clap, AppSettings};
use whoami;

use crate::repl::OutputMode;
use crate::commands::parser::Common;
use crate::self_install;
use crate::server;
use edgedb_client::builder::Builder;


#[derive(Clap, Debug)]
#[clap(version=clap::crate_version!())]
struct TmpOptions {
    /// Host of the EdgeDB instance
    #[clap(short="H", long, help_heading=Some("CONNECTION OPTIONS"))]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[clap(short="P", long, help_heading=Some("CONNECTION OPTIONS"))]
    pub port: Option<u16>,

    /// User name of the EdgeDB user
    #[clap(short="u", long, help_heading=Some("CONNECTION OPTIONS"))]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short="d", long, help_heading=Some("CONNECTION OPTIONS"))]
    pub database: Option<String>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default
    #[clap(long, help_heading=Some("CONNECTION OPTIONS"))]
    pub admin: bool,

    /// Ask for password on the terminal (TTY)
    #[clap(long, help_heading=Some("CONNECTION OPTIONS"))]
    pub password: bool,

    /// Don't ask for password
    #[clap(long, help_heading=Some("CONNECTION OPTIONS"))]
    pub no_password: bool,

    /// Read the password from stdin rather than TTY (useful for scripts)
    #[clap(long, help_heading=Some("CONNECTION OPTIONS"))]
    pub password_from_stdin: bool,

    /// In case EdgeDB connection can't be established, retry up to N seconds.
    #[clap(long, name="N", help_heading=Some("CONNECTION OPTIONS"),
                parse(try_from_str=humantime::parse_duration))]
    pub wait_until_available: Option<Duration>,

    /// Local instance name created with `edgedb server init` to connect to
    /// (overrides host and port)
    #[clap(short="I", long, help_heading=Some("CONNECTION OPTIONS"))]
    pub instance: Option<String>,

    #[clap(long, help_heading=Some("DEBUG OPTIONS"))]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
    pub debug_print_frames: bool,

    #[clap(long, help_heading=Some("DEBUG OPTIONS"))]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
    pub debug_print_descriptors: bool,

    #[clap(long, help_heading=Some("DEBUG OPTIONS"))]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
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

#[derive(Clap, Clone, Debug)]
pub enum Command {
    AlterRole(RoleParams),
    CreateSuperuserRole(RoleParams),
    DropRole(RoleName),
    Query(Query),
    Server(server::options::ServerCommand),
    #[clap(setting=AppSettings::Hidden, name="_self_install")]
    _SelfInstall(self_install::SelfInstall),
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
    pub conn_params: Builder,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_mode: OutputMode,
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
            Some(password)
        } else if tmp.no_password {
            None
        } else if tmp.password {
            Some(rpassword::read_password_from_tty(
                    Some(&format!("Password for '{}': ",
                                  user.escape_default())))
                 .unwrap_or_else(|e| {
                     eprintln!("Error reading password: {:#}", e);
                     exit(1);
                }))
        } else {
            match env::var("EDGEDB_PASSWORD") {
                Ok(p) => Some(p),
                Err(_) => None,
            }
        };

        let mut conn_params = Builder::new();
        conn_params.user(user);
        password.map(|password| conn_params.password(password));
        conn_params.database(database);
        tmp.wait_until_available.map(|w| conn_params.wait_until_available(w));

        let unix_host = host.contains("/");
        if let Some(name) = tmp.instance {
            let path = server::get_instance(&name)
                .and_then(|inst| inst.get_socket(admin))
                .unwrap_or_else(|e| {
                    eprintln!("Could not find instance {:?}: {:#}.\n\
                        Note: only instances created by `edgedb server init` \
                        are supported by `--instance` argument.", name, e);
                    exit(1);
                });
            conn_params.unix_addr(path);
        } else if admin || unix_host {
            let prefix = if unix_host {
                &host
            } else {
                "/var/run/edgedb"
            };
            let path = if prefix.contains(".s.EDGEDB") {
                // it's the full path
                prefix.into()
            } else {
                if admin {
                    format!("{}/.s.EDGEDB.admin.{}", prefix, port)
                } else {
                    format!("{}/.s.EDGEDB.{}", prefix, port)
                }
            };
            conn_params.unix_addr(path);
        } else {
            conn_params.tcp_addr(host, port);
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
            conn_params,
            interactive,
            subcommand,
            debug_print_frames: tmp.debug_print_frames,
            debug_print_descriptors: tmp.debug_print_descriptors,
            debug_print_codecs: tmp.debug_print_codecs,
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
