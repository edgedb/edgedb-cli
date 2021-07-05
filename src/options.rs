use std::env;
use std::path::PathBuf;
use std::process::exit;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use anymap::AnyMap;
use atty;
use clap::{ValueHint};
use colorful::Colorful;
use edgedb_client::Builder;
use edgedb_cli_derive::EdbClap;
use fs_err as fs;

use crate::commands::parser::Common;
use crate::connect::Connector;
use crate::credentials::get_connector;
use crate::hint::HintExt;
use crate::project;
use crate::repl::OutputMode;
use crate::self_install;
use crate::self_migrate;
use crate::self_upgrade;
use crate::server;

pub mod describe;

static CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";

pub trait PropagateArgs {
    fn propagate_args(&self, dest: &mut AnyMap, matches: &clap::ArgMatches);
}

#[derive(EdbClap, Debug)]
pub struct ConnectionOptions {
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

    /// Certificate to match server against
    ///
    /// This might either be full self-signed server certificate or certificate
    /// authority (CA) certificate that server certificate is signed with.
    pub tls_cert_file: Option<PathBuf>,


    /// Verify hostname of the server using provided certificate
    ///
    /// It's useful when certificate authority (CA) is used for handling
    /// certificate and usually not used for self-signed certificates.
    ///
    /// By default it's enabled when no specific certificate is present
    /// (via `--tls-cert-file` or in credentials JSON file)
    #[clap(long)]
    pub tls_verify_hostname: bool,

    /// Do not verify hostname of the server
    ///
    /// This allows using any certificate for any hostname. However,
    /// certificate must be present and match certificate specified with
    /// `--tls-cert-file` or credentials file or signed by one of the root
    /// certificate authorities.
    #[clap(long)]
    pub no_tls_verify_hostname: bool,

    /// Local instance name created with `edgedb server init` to connect to
    /// (overrides host and port)
    #[clap(short='I', long, help_heading=Some("CONNECTION OPTIONS"))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub instance: Option<String>,
}

#[derive(EdbClap, Debug)]
#[edb(main)]
#[clap(version=clap::crate_version!())]
pub struct RawOptions {

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

    #[edb(inheritable)]
    pub conn: ConnectionOptions,

    #[clap(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Authenticate to a remote instance
    Authenticate(Authenticate),
    #[clap(flatten)]
    Common(Common),
    /// Execute EdgeQL query
    #[edb(inherit(ConnectionOptions), hidden)]
    Query(Query),
    /// Manage local server installations
    #[edb(expand_help)]
    Server(server::options::ServerCommand),
    /// Manage project installation
    #[edb(expand_help)]
    Project(project::options::ProjectCommand),
    /// Generate shell completions
    #[clap(name="_gen_completions")]
    #[edb(hidden)]
    _GenCompletions(self_install::GenCompletions),
    /// Self-installation commands
    #[clap(name="self")]
    #[edb(expand_help)]
    SelfCommand(SelfCommand),
    /// Install server
    #[clap(name="_self_install")]
    #[edb(hidden)]
    _SelfInstall(self_install::SelfInstall),
    #[clap(name="_generate_dev_cert")]
    #[edb(hidden)]
    _GenDevCert(GenerateDevCert)
}

#[derive(EdbClap, Clone, Debug)]
pub struct SelfCommand {
    #[clap(subcommand)]
    pub subcommand: SelfSubcommand,
}

#[derive(EdbClap, Clone, Debug)]
pub enum SelfSubcommand {
    /// Upgrade this edgedb binary
    Upgrade(self_upgrade::SelfUpgrade),
    /// Install server
    Install(self_install::SelfInstall),
    /// Migrate files from `~/.edgedb` to new directory layout
    #[edb(hidden)]
    Migrate(self_migrate::SelfMigrate),
}

#[derive(EdbClap, Clone, Debug)]
pub struct Query {
    pub queries: Vec<String>,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(long_about = "Authenticate to a remote EdgeDB instance and
assign an instance name to simplify future connections.")]
pub struct Authenticate {
    /// Specify a new instance name for the remote server. If not
    /// present, the name will be interactively asked.
    pub name: Option<String>,

    /// Run in non-interactive mode (accepting all defaults)
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Clone, Debug)]
pub struct GenerateDevCert {
    /// Specify a path to store the generated certificates
    pub path: String,
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

fn make_subcommand_help<T: describe::Describe>() -> String {
    use std::fmt::Write;

    let mut buf = String::with_capacity(4096);

    write!(&mut buf, "SUBCOMMANDS:\n").unwrap();
    let descr = T::describe();
    let mut empty_line = true;

    for cmd in descr.subcommands() {
        let cdescr = cmd.describe();
        if cmd.hidden {
            continue;
        }
        if cmd.expand_help {
            if !empty_line {
                buf.push('\n');
            }
            for subcmd in cdescr.subcommands() {
                let sdescr = subcmd.describe();
                if subcmd.hidden {
                    continue;
                }
                writeln!(&mut buf, "    {:24} {}",
                    format!("{} {}", cmd.name, subcmd.name),
                    sdescr.help_title,
                ).unwrap();
            }
            buf.push('\n');
            empty_line = true;
        } else {
            writeln!(&mut buf, "    {:24} {}",
                cmd.name, cdescr.help_title).unwrap();
            empty_line = false;
        }
    }
    buf.truncate(buf.trim_end().len());

    return buf;
}

fn update_help(mut app: clap::App) -> clap::App {
    let sub_cmd = make_subcommand_help::<RawOptions>();
    let mut help = Vec::with_capacity(2048);
    app.write_help(&mut help).unwrap();

    let subcmd_index = std::str::from_utf8(&help).unwrap()
        .find("SUBCOMMANDS:").unwrap();
    help.truncate(subcmd_index);
    help.extend(sub_cmd.as_bytes());
    let help = std::str::from_utf8(Vec::leak(help)).unwrap();
    return app.override_help(help);
}

fn get_matches(app: clap::App) -> clap::ArgMatches {
    use clap::ErrorKind::*;

    match app.try_get_matches() {
        Ok(matches) => matches,
        Err(e) => {
            match e.kind {
                UnknownArgument | InvalidSubcommand => {
                    let new_name = match &e.info[0][..] {
                         "configure" => "config",
                         "create-database" => "database create",
                         "create-migration" => "migration create",
                         "list-aliases" => "list aliases",
                         "list-casts" => "list casts",
                         "list-databases" => "list databases",
                         "list-indexes" => "list indexes",
                         "list-object-types" => "list types",
                         "list-scalar-types" => "list scalars",
                         "list-roles" => "list roles",
                         "migration-log" => "migration log",
                         "self-upgrade" => "self upgrade",
                         "show-status" => "migration status",
                         _ => e.exit(),
                    };
                    let error = "error:".bold().red();
                    let cmd = e.info[0][..].green();
                    let instead = format!("edgedb {}", new_name).green();
                    eprintln!("\
                        {error} The subcommand '{cmd}' was renamed\n\
                        \n        \
                            Use '{instead}' instead\
                    ", error=error, cmd=cmd, instead=instead);
                    exit(1);
                }
                _ => {}
            }
            e.exit();
        }
    }
}

impl Options {
    pub fn from_args_and_env() -> anyhow::Result<Options> {
        let app = <RawOptions as clap::IntoApp>::into_app();
        let app = update_help(app);
        let matches = get_matches(app);
        let tmp = <RawOptions as clap::FromArgMatches>
            ::from_arg_matches(&matches);

        // TODO(pc) add option to force interactive mode not on a tty (tests)
        let interactive = tmp.query.is_none()
            && tmp.subcommand.is_none()
            && atty::is(atty::Stream::Stdin);

        let mut builder = conn_params(&tmp.conn);
        if let (Some(Command::Authenticate(auth)), None) = (&tmp.subcommand, &tmp.query) {
            if builder.is_err() {
                builder = Ok(Builder::new());
                load_tls_options(&tmp.conn, builder.as_mut().unwrap())?;
            }
            server::authenticate::prompt_conn_params(
                &tmp.conn, builder.as_mut().unwrap(), auth
            )?;
        }
        let mut conn_params = Connector::new(builder);
        let password = if tmp.conn.password_from_stdin {
            let password = rpassword::read_password()
                .expect("password can be read");
            Some(password)
        } else if tmp.conn.no_password {
            None
        } else if tmp.conn.password {
            let user = conn_params.get()?.get_user();
            Some(rpassword::read_password_from_tty(
                    Some(&format!("Password for '{}': ",
                                  user.escape_default())))
                 .context("error reading password")?)
        } else {
            get_env("EDGEDB_PASSWORD")?
        };
        conn_params.modify(|params| {
            password.map(|password| params.password(password));
            tmp.conn.wait_until_available
                .map(|w| params.wait_until_available(w));
            tmp.conn.connect_timeout.map(|t| params.connect_timeout(t));
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

fn is_env(name: &str) -> bool {
    env::var_os(name).map(|v| !v.is_empty()).unwrap_or(false)
}

fn get_env(name: &str) -> anyhow::Result<Option<String>> {
    match env::var(name) {
        Ok(v) if v.is_empty() => Ok(None),
        Ok(v) => Ok(Some(v)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(e) => {
            Err(e).with_context(|| {
                format!("Cannot decode environment variable {:?}", name)
            })
        }
    }
}

fn env_fallback<T: FromStr>(value: Option<T>, name: &str)
    -> anyhow::Result<Option<T>>
    where <T as FromStr>::Err: std::error::Error + Send + Sync + 'static
{
    match value {
        Some(value) => Ok(Some(value)),
        None => match get_env(name) {
            Ok(Some(value)) => {
                Ok(Some(value.parse().with_context(|| {
                    format!("Cannot parse environment variable {:?}", name)
                })?))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        },
    }
}

fn conn_params(tmp: &ConnectionOptions) -> anyhow::Result<Builder> {
    let instance = if let Some(dsn) = &tmp.dsn {
        return Ok(Builder::from_dsn(dsn)?);
    } else if let Some(inst) = get_env("EDGEDB_INSTANCE")? {
        if inst.starts_with("edgedb://") {
            return Ok(Builder::from_dsn(&inst)?);
        } else {
            Some(inst)
        }
    } else if tmp.instance.is_some() ||
            tmp.host.is_some() || tmp.port.is_some() ||
            is_env("EDGEDB_HOST") ||
            is_env("EDGEDB_PORT")
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
            .context("error reading project settings")
            .hint(CONNECTION_ARG_HINT)?
        )
    };

    let admin = tmp.admin;
    let user = env_fallback(tmp.user.clone(), "EDGEDB_USER")?;
    let host = env_fallback(tmp.host.clone(), "EDGEDB_HOST")?;
    let port = env_fallback(tmp.port, "EDGEDB_PORT")?;
    let database = env_fallback(tmp.database.clone(), "EDGEDB_DATABASE")?;

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
            load_tls_options(tmp, &mut conn_params)?;
        }
    }
    Ok(conn_params)
}

fn load_tls_options(options: &ConnectionOptions, builder: &mut Builder)
    -> anyhow::Result<()>
{
    if let Some(cert_file) = &options.tls_cert_file {
        let data = fs::read_to_string(cert_file)?;
        builder.pem_certificates(&data)?;
    }
    if options.no_tls_verify_hostname {
        builder.verify_hostname(false);
    }
    if options.tls_verify_hostname {
        builder.verify_hostname(true);
    }
    Ok(())
}
