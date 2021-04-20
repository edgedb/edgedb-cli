use std::env;
use std::time::Duration;
use std::fs;

use anyhow::Context;
use atty;
use clap::{Clap, AppSettings, ValueHint};
use edgedb_client::Builder;

use crate::commands::parser::Common;
use crate::connect::Connector;
use crate::credentials::get_connector;
use crate::hint::HintExt;
use crate::project;
use crate::repl::OutputMode;
use crate::self_install;
use crate::self_upgrade;
use crate::server;


static CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";


#[derive(Clap, Debug)]
#[clap(version=clap::crate_version!())]
pub struct RawOptions {
    /// DSN for EdgeDB to connect to (overrides all other options
    /// except password)
    #[clap(long, help_heading=Some("CONNECTION OPTIONS"))]
    pub dsn: Option<String>,

    /// Host of the EdgeDB instance
    #[clap(short='H', long, help_heading=Some("CONNECTION OPTIONS"))]
    #[clap(value_hint=ValueHint::Hostname)]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[clap(short='P', long, help_heading=Some("CONNECTION OPTIONS"))]
    pub port: Option<u16>,

    /// User name of the EdgeDB user
    #[clap(short='u', long, help_heading=Some("CONNECTION OPTIONS"))]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short='d', long, help_heading=Some("CONNECTION OPTIONS"))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete database
    pub database: Option<String>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default. (DEPRECATED)
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

    /// In case EdgeDB connection can't be established, retry up to
    /// WAIT_TIME (e.g. '30s').
    #[clap(long, name="WAIT_TIME", help_heading=Some("CONNECTION OPTIONS"),
                parse(try_from_str=humantime::parse_duration))]
    pub wait_until_available: Option<Duration>,

    /// In case EdgeDB doesn't respond for a TIMEOUT, fail
    /// (or retry if --wait-until-available is also specified). Default '10s'.
    #[clap(long, name="TIMEOUT", help_heading=Some("CONNECTION OPTIONS"),
           parse(try_from_str=humantime::parse_duration))]
    pub connect_timeout: Option<Duration>,

    /// Local instance name created with `edgedb server init` to connect to
    /// (overrides host and port)
    #[clap(short='I', long, help_heading=Some("CONNECTION OPTIONS"))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
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
    #[clap(short='t', long, overrides_with="json")]
    pub tab_separated: bool,

    /// JSON output for the queries (single JSON list per query)
    #[clap(short='j', long, overrides_with="tab_separated")]
    pub json: bool,

    /// Execute a query instead of starting REPL (alias to `edgedb query`)
    #[clap(short='c')]
    pub query: Option<String>,

    /// Disable version check
    #[clap(long)]
    pub no_version_check: bool,

    #[clap(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(Clap, Clone, Debug)]
pub enum Command {
    /// Change role parameters
    AlterRole(RoleParams),
    /// Create a new role
    CreateSuperuserRole(RoleParams),
    /// Delete a role
    DropRole(RoleName),
    /// Execute EdgeQL query
    Query(Query),
    /// Manage local server installations
    Server(server::options::ServerCommand),
    /// Manage project installation
    Project(project::options::ProjectCommand),
    /// Install server
    #[clap(setting=AppSettings::Hidden, name="_self_install")]
    _SelfInstall(self_install::SelfInstall),
    /// Generate shell completions
    #[clap(setting=AppSettings::Hidden, name="_gen_completions")]
    _GenCompletions(self_install::GenCompletions),
    /// Upgrade this edgedb binary
    SelfUpgrade(self_upgrade::SelfUpgrade),
    #[clap(flatten)]
    Common(Common),
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct Query {
    pub queries: Vec<String>,
}

#[derive(Clap, Clone, Debug)]
#[clap(setting=AppSettings::DisableVersionFlag)]
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
#[clap(setting=AppSettings::DisableVersionFlag)]
pub struct RoleName {
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub conn_params: Connector,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_mode: OutputMode,
    pub no_version_check: bool,
}

impl Options {
    pub fn from_args_and_env() -> anyhow::Result<Options> {
        let tmp = RawOptions::parse();
        // TODO(pc) add option to force interactive mode not on a tty (tests)
        let interactive = tmp.query.is_none()
            && tmp.subcommand.is_none()
            && atty::is(atty::Stream::Stdin);
        let mut conn_params = Connector::new(conn_params(&tmp));
        let password = if tmp.password_from_stdin {
            let password = rpassword::read_password()
                .expect("password can be read");
            Some(password)
        } else if tmp.no_password {
            None
        } else if tmp.password {
            let user = conn_params.get()?.get_user();
            Some(rpassword::read_password_from_tty(
                    Some(&format!("Password for '{}': ",
                                  user.escape_default())))
                 .context("error reading password")?)
        } else {
            match env::var("EDGEDB_PASSWORD") {
                Ok(p) => Some(p),
                Err(_) => None,
            }
        };
        conn_params.modify(|params| {
            password.map(|password| params.password(password));
            tmp.wait_until_available.map(|w| params.wait_until_available(w));
            tmp.connect_timeout.map(|t| params.connect_timeout(t));
        });

        let subcommand = if let Some(query) = tmp.query {
            if tmp.subcommand.is_some() {
                anyhow::bail!(
                    "Option `-c` conflicts with specifying subcommand");
            } else {
                Some(Command::Query(Query {
                    queries: vec![query],
                }))
            }
        } else {
            tmp.subcommand
        };

        Ok(Options {
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
            no_version_check: tmp.no_version_check,
        })
    }
}

fn conn_params(tmp: &RawOptions) -> anyhow::Result<Builder> {
    let instance = if let Some(dsn) = &tmp.dsn {
        return Ok(Builder::from_dsn(dsn)?);
    } else if tmp.instance.is_some() ||
            tmp.host.is_some() || tmp.port.is_some() ||
            env::var("EDGEDB_HOST").is_ok() ||
            env::var("EDGEDB_PORT").is_ok()
    {
        tmp.instance.clone()
    } else {
        let dir = env::current_dir()
            .context("cannot determine current dir")
            .hint(CONNECTION_ARG_HINT)?;
        let config_dir = project::project_dir_opt(Some(&dir))
            .context("error searching for `edgedb.toml`")
            .hint(CONNECTION_ARG_HINT)?
            .ok_or_else(|| {
                anyhow::anyhow!("no `edgedb.toml` found \
                    and no connection options are specified")
            })
            .hint(CONNECTION_ARG_HINT)?;
        let dir = project::stash_path(&config_dir)?;
        Some(
            fs::read_to_string(dir.join("instance-name"))
            .context("error reading project settings")?
        )
    };

    let admin = tmp.admin;
    let user = tmp.user.clone().or_else(|| env::var("EDGEDB_USER").ok());
    let host = tmp.host.clone().or_else(|| env::var("EDGEDB_HOST").ok());
    let port = tmp.port.or_else(|| {
        env::var("EDGEDB_PORT").ok().and_then(|x| x.parse().ok())
    });
    let database = tmp.database.clone()
        .or_else(|| env::var("EDGEDB_DATABASE").ok());

    let mut conn_params = Builder::new();
    if let Some(name) = &instance {
        conn_params = get_connector(name)?;
        user.map(|user| conn_params.user(user));
        database.map(|database| conn_params.database(database));
    } else {
        user.as_ref().map(|user| conn_params.user(user));
        database.as_ref().map(|db| conn_params.database(db));
        let host = host.unwrap_or_else(|| String::from("localhost"));
        let port = port.unwrap_or(5656);
        let unix_host = host.contains("/");
        if admin || unix_host {
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
        }
    }
    Ok(conn_params)
}
