use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
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

use crate::cli;
use crate::cli::options::CliCommand;
use crate::commands::parser::Common;
use crate::commands::ExitCode;
use crate::connect::Connector;
use crate::credentials::get_connector;
use crate::hint::HintExt;
use crate::project;
use crate::repl::OutputFormat;
use crate::server;

pub mod describe;

const MAX_TERM_WIDTH: usize = 90;
const MIN_TERM_WIDTH: usize = 50;

static CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";

const CONN_OPTIONS_GROUP: &str =
    "CONNECTION OPTIONS (`edgedb --help-connect` to see the full list)";

const EDGEDB_ABOUT: &str = "\
    Use the `edgedb` command-line tool to spin up local instances, \
    manage EdgeDB projects, create and apply migrations, and more. \
    \n\n\
    Running `edgedb` without a subcommand opens an interactive shell.";

pub trait PropagateArgs {
    fn propagate_args(&self, dest: &mut AnyMap, matches: &clap::ArgMatches);
}

#[derive(EdbClap, Clone, Debug)]
#[clap(setting=clap::AppSettings::DeriveDisplayOrder)]
pub struct ConnectionOptions {
    /// Local instance name created with `edgedb instance create` to connect to
    /// (overrides host and port)
    #[clap(short='I', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub instance: Option<String>,

    /// DSN for EdgeDB to connect to (overrides all other options
    /// except password)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    pub dsn: Option<String>,

    /// Host of the EdgeDB instance
    #[clap(short='H', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Hostname)]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[clap(short='P', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub port: Option<u16>,

    /// User name of the EdgeDB user
    #[clap(short='u', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short='d', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete database
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub database: Option<String>,

    /// Ask for password on the terminal (TTY)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub password: bool,

    /// Don't ask for password
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub no_password: bool,

    /// Read the password from stdin rather than TTY (useful for scripts)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub password_from_stdin: bool,

    /// Certificate to match server against
    ///
    /// This might either be full self-signed server certificate or certificate
    /// authority (CA) certificate that server certificate is signed with.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub tls_ca_file: Option<PathBuf>,


    /// Verify hostname of the server using provided certificate
    ///
    /// It's useful when certificate authority (CA) is used for handling
    /// certificate and usually not used for self-signed certificates.
    ///
    /// By default it's enabled when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub tls_verify_hostname: bool,

    /// Do not verify hostname of the server
    ///
    /// This allows using any certificate for any hostname. However,
    /// certificate must be present and match certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub no_tls_verify_hostname: bool,

    /// In case EdgeDB connection can't be established, retry up to
    /// WAIT_TIME (e.g. '30s').
    #[clap(long, name="WAIT_TIME", help_heading=Some(CONN_OPTIONS_GROUP),
                parse(try_from_str=humantime::parse_duration))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub wait_until_available: Option<Duration>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub admin: bool,

    /// In case EdgeDB doesn't respond for a TIMEOUT, fail
    /// (or retry if `--wait-until-available` is also specified). Default '10s'.
    #[clap(long, name="TIMEOUT", help_heading=Some(CONN_OPTIONS_GROUP),
           parse(try_from_str=humantime::parse_duration))]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub connect_timeout: Option<Duration>,
}

#[derive(EdbClap, Debug)]
#[edb(main)]
#[clap(setting=clap::AppSettings::DisableVersionFlag)]
pub struct RawOptions {
    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
    pub debug_print_frames: bool,

    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
    pub debug_print_descriptors: bool,

    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"),
        clap(setting=clap::ArgSettings::Hidden))]
    pub debug_print_codecs: bool,

    /// Print all available connection options
    /// for the interactive shell and subcommands
    #[clap(long)]
    pub help_connect: bool,

    /// Tab-separated output of the queries
    #[clap(short='t', long, overrides_with="json",
           setting=clap::ArgSettings::Hidden)]
    pub tab_separated: bool,
    /// JSON output for the queries (single JSON list per query)
    #[clap(short='j', long, overrides_with="tab_separated",
           setting=clap::ArgSettings::Hidden)]
    pub json: bool,
    /// Execute a query instead of starting REPL
    #[clap(short='c', setting=clap::ArgSettings::Hidden)]
    pub query: Option<String>,

    /// Show command-line tool version
    #[clap(short='V', long="version")]
    pub print_version: bool,

    // (deprecated in favor of "no_cli_update_check")
    #[clap(long)]
    #[clap(setting=clap::ArgSettings::Hidden)]
    pub no_version_check: bool,

    /// Disable checking if a new version of CLI is available
    #[clap(long)]
    pub no_cli_update_check: bool,

    #[edb(inheritable)]
    pub conn: ConnectionOptions,

    #[clap(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    #[clap(flatten)]
    Common(Common),
    /// Execute EdgeQL queries
    #[edb(inherit(ConnectionOptions))]
    Query(Query),
    /// Show information about the EdgeDB installation
    Info,
    /// Manage project installation
    #[edb(expand_help)]
    Project(project::options::ProjectCommand),
    /// Manage local EdgeDB instances
    #[edb(expand_help)]
    Instance(server::options::ServerInstanceCommand),
    /// Manage local EdgeDB installations
    Server(server::options::ServerCommand),
    /// Generate shell completions
    #[clap(name="_gen_completions")]
    #[edb(hidden)]
    _GenCompletions(cli::install::GenCompletions),
    /// Self-installation commands
    #[clap(name="cli")]
    #[edb(expand_help)]
    CliCommand(CliCommand),
    /// Install EdgeDB
    #[clap(name="_self_install")]
    #[edb(hidden)]
    _SelfInstall(cli::install::CliInstall),
}

#[derive(EdbClap, Clone, Debug)]
pub struct Query {
    /// Output format: `json`, `json-pretty`, `json-lines`, `tab-separated`.
    /// Default is `json-pretty`.
    // todo: can't use `clap(default='json-pretty')` just yet, as we
    // need to see if the user did actually specify some output
    // format or not. We need that to support the now deprecated
    // --json and --tab-separated top-level options.
    #[clap(short='F', long)]
    pub output_format: Option<OutputFormat>,

    /// Filename to execute queries from.
    /// Pass `--file -` to execute queries from stdin.
    #[clap(short='f', long)]
    pub file: Option<String>,

    pub queries: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub conn_options: ConnectionOptions,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_format: OutputFormat,
    pub no_cli_update_check: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("error searching for `edgedb.toml`")]
pub struct ProjectNotFound(#[source] pub anyhow::Error);

fn say_option_is_deprecated(option_name: &str, suggestion: &str) {
    let error = "warning:".bold().light_yellow();
    let instead = suggestion.green();
    eprintln!("\
        {error} The '{opt}' option is deprecated.\n\
        \n         \
            Use '{instead}' instead.\
        \n\
    ", error=error, opt=option_name.green(), instead=instead);
}

fn make_subcommand_help<T: describe::Describe>() -> String {
    use std::fmt::Write;

    let width = term_width();

    // When the terminal is wider than 82 characters clap aligns
    // the flags description text to the right of the flag name,
    // when it is narrower than 82, the description goes below
    // the option name.  We want to align the subcommand description
    // with the option description, hence there's some hand-tuning
    // of the padding here.
    let padding: usize = if width > 82 { 28 } else { 24 };

    let extra_padding: usize = 4 + 1;
    let details_width: usize = width - padding - extra_padding;

    let wrap = |text: &str| {
        if text.len() <= details_width {
            return text.to_string();
        }

        let text = textwrap::fill(text, details_width);
        let mut lines = text.lines();
        let mut new_lines = vec![lines.nth(0).unwrap().to_string()];
        for line in lines {
            new_lines.push(
                format!("    {:padding$} {}", " ", line, padding=padding)
            );
        }

        new_lines.join("\n")
    };

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
                writeln!(&mut buf, "    {:padding$} {}",
                    format!("{} {}", cmd.name, subcmd.name),
                    wrap(sdescr.help_title),
                    padding=padding
                ).unwrap();
            }
            buf.push('\n');
            empty_line = true;
        } else {
            writeln!(&mut buf, "    {:padding$} {}",
                cmd.name, wrap(cdescr.help_title),
                padding=padding
            ).unwrap();
            empty_line = false;
        }
    }
    buf.truncate(buf.trim_end().len());

    return buf;
}

fn update_main_help(mut app: clap::App) -> clap::App {
    let sub_cmd = make_subcommand_help::<RawOptions>();
    let mut help = Vec::with_capacity(2048);

    app.write_help(&mut help).unwrap();

    let help = String::from_utf8(help).unwrap();
    let subcmd_index = help.find("SUBCOMMANDS:").unwrap();
    let mut help = help[..subcmd_index].replacen("edgedb", "EdgeDB CLI", 1);
    help.push_str(&sub_cmd);

    let help = std::str::from_utf8(Vec::leak(help.into())).unwrap();
    return app.override_help(help);
}

fn print_full_connection_options() {
    let app = <ConnectionOptions as clap::IntoApp>::into_app();

    let mut new_app = clap::App::new("edgedb-connect")
                      .setting(clap::AppSettings::DeriveDisplayOrder)
                      .term_width(term_width());

    for arg in app.get_arguments() {
        let arg_name = arg.get_name();
        if arg_name == "help" || arg_name == "version" || arg_name == "admin" {
            continue
        }
        let new_arg = arg.clone().unset_setting(clap::ArgSettings::Hidden);
        new_app = new_app.arg(new_arg);
    }

    let mut help = Vec::with_capacity(2048);

    // "Long help" has more whitespace and is much more readable
    // for the many options we have in the connection group.
    new_app.write_long_help(&mut help).unwrap();

    let help = String::from_utf8(help).unwrap();
    let subcmd_index = help.find(CONN_OPTIONS_GROUP).unwrap();
    let slice_from = subcmd_index + CONN_OPTIONS_GROUP.len() + 2;
    let help = &help[slice_from..];

    println!("CONNECTION OPTIONS (the full list):\n");
    println!("{}", help);
}

fn get_matches(app: clap::App) -> clap::ArgMatches {
    use clap::ErrorKind::*;

    match app.try_get_matches() {
        Ok(matches) => matches,
        Err(e) => {
            match e.kind {
                UnknownArgument | InvalidSubcommand => {
                    let mismatch_cmd = &e.info[0][..];
                    match get_deprecated_matches(mismatch_cmd) {
                        Some(matches) => matches,
                        None => e.exit(),
                    }
                }
                _ => e.exit(),
            }
        }
    }
}

fn get_deprecated_matches(mismatch_cmd: &str) -> Option<clap::ArgMatches> {
    let mut args = env::args_os().skip(1);
    let old_name;
    let skip;
    let new_name = match args.next() {
        Some(first_cmd) if first_cmd == "server" => match args.next() {
            Some(second_cmd) if second_cmd == mismatch_cmd => {
                old_name = format!("server {}", mismatch_cmd);
                skip = 3;
                match mismatch_cmd {
                    "init" => "instance create",
                    "status" => "instance status",
                    "start" => "instance start",
                    "stop" => "instance stop",
                    "restart" => "instance restart",
                    "destroy" => "instance destroy",
                    "logs" => "instance logs",
                    "upgrade" => "instance upgrade",
                    _ => return None,
                }
            }
            _ => return None,
        }
        Some(first_cmd) if first_cmd == mismatch_cmd => {
            old_name = mismatch_cmd.into();
            skip = 2;
            match mismatch_cmd {
                "create-database" => "database create",
                "create-migration" => "migration create",
                "list-aliases" => "list aliases",
                "list-casts" => "list casts",
                "list-databases" => "list databases",
                "list-indexes" => "list indexes",
                "list-modules" => "list modules",
                "list-object-types" => "list types",
                "list-scalar-types" => "list scalars",
                "list-roles" => "list roles",
                "migration-log" => "migration log",
                "self-upgrade" => "cli upgrade",
                "show-status" => "migration status",
                _ => return None,
            }
        }
        _ => return None,
    };
    let error = "warning:".bold().light_yellow();
    let instead = format!("edgedb {}", new_name).green();
    eprintln!("\
        {error} The '{cmd}' subcommand was renamed.\n\
        \n         \
            Use '{instead}' instead.\
        \n\
    ", error=error, cmd=old_name.green(), instead=instead);
    let new_args: Vec<OsString> = env::args_os().take(1).chain(
        new_name.split(" ").map(|x| x.into())
    ).chain(
        env::args_os().skip(skip)
    ).collect();
    let app = <RawOptions as clap::IntoApp>::into_app();
    Some(app.get_matches_from(new_args))
}

fn term_width() -> usize {
    use std::cmp;

    // clap::App::max_term_width() works poorly in conjunction
    // with  clap::App::term_width(); it appears that one call
    // disables the effect of the other. Therefore we want to
    // calculate the acceptable term width ourselves and use
    // that to configure clap and to render subcommands help.

    cmp::max(
        cmp::min(textwrap::termwidth(), MAX_TERM_WIDTH),
        MIN_TERM_WIDTH
    )
}

impl Options {
    pub fn from_args_and_env() -> anyhow::Result<Options> {
        let app = <RawOptions as clap::IntoApp>::into_app()
                  .name("edgedb")
                  .about(EDGEDB_ABOUT)
                  .term_width(term_width());
        let app = update_main_help(app);
        let matches = get_matches(app);
        let tmp: RawOptions = <RawOptions as clap::FromArgMatches>
            ::from_arg_matches(&matches);

        if tmp.help_connect {
            print_full_connection_options();
            return Err(ExitCode::new(0).into());
        }

        if tmp.print_version {
            println!("EdgeDB CLI {}", clap::crate_version!());
            return Err(ExitCode::new(0).into());
        }

        if tmp.subcommand.is_some() && tmp.query.is_some() {
            anyhow::bail!(
                "Option `-c` conflicts with specifying a subcommand"
            );
        }

        // TODO(pc) add option to force interactive mode not on a tty (tests)
        let interactive = tmp.query.is_none()
            && tmp.subcommand.is_none()
            && atty::is(atty::Stream::Stdin);

        if tmp.json {
            say_option_is_deprecated(
                "--json",
                "edgedb query --output-format=json");
        }
        if tmp.tab_separated {
            say_option_is_deprecated(
                "--tab-separated",
                "edgedb query --output-format=tab-separated");
        }
        let subcommand = if let Some(query) = tmp.query {
            say_option_is_deprecated("-c", "edgedb query");
            let output_format = if tmp.json {
                Some(OutputFormat::Json)
            } else if tmp.tab_separated {
                Some(OutputFormat::TabSeparated)
            } else {
                Some(OutputFormat::JsonPretty)
            };
            Some(Command::Query(Query {
                queries: Some(vec![query]),
                output_format,
                file: None,
            }))
        } else {
            tmp.subcommand
        };

        let mut no_cli_update_check = tmp.no_cli_update_check;
        if tmp.no_version_check {
            no_cli_update_check = true;
            eprintln!("\
                {error} The '--no-version-check' option was renamed.\n\
                \n         \
                    Use '--no-cli-update-check' instead.\
                \n\
            ", error="warning:".bold().light_yellow());
        }

        Ok(Options {
            conn_options: tmp.conn,
            interactive,
            subcommand,
            debug_print_frames: tmp.debug_print_frames,
            debug_print_descriptors: tmp.debug_print_descriptors,
            debug_print_codecs: tmp.debug_print_codecs,
            output_format: if tmp.tab_separated {
                OutputFormat::TabSeparated
            } else if tmp.json {
                OutputFormat::Json
            } else if interactive {
                OutputFormat::Default
            } else {
                OutputFormat::JsonPretty
            },
            no_cli_update_check,
        })
    }

    pub fn create_connector(&self) -> anyhow::Result<Connector> {
        let conn = &self.conn_options;
        let mut conn_params = Connector::new(conn_params(conn));
        let password = if conn.password_from_stdin {
            let password = rpassword::read_password()
                .expect("password cannot be read");
            Some(password)
        } else if conn.no_password {
            None
        } else if conn.password {
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
            conn.wait_until_available
                .map(|w| params.wait_until_available(w));
            conn.connect_timeout.map(|t| params.connect_timeout(t));
        });
        Ok(conn_params)
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

fn parse_port_env() -> anyhow::Result<(Option<String>, Option<u16>)> {
    match get_env("EDGEDB_PORT") {
        Ok(Some(value)) => if let Some(value) = value.strip_prefix("tcp://") {
            // #389: `docker run --link=edgedb edgedb/edgedb-cli` will set env
            // `EDGEDB_PORT` to `tcp://<host>:<port>` - we take it as default.
            if let Some((host, port)) = value.split_once(":") {
                let port = port.parse().with_context(|| {
                    format!("Cannot parse port in Docker URI: EDGEDB_PORT")
                })?;
                Ok((Some(host.into()), Some(port)))
            } else {
                anyhow::bail!("Port not found in Docker URI: EDGEDB_PORT");
            }
        } else {
            let port = value.parse().with_context(|| {
                format!("Cannot parse environment variable: EDGEDB_PORT")
            })?;
            Ok((None, Some(port)))
        }
        Ok(None) => Ok((None, None)),
        Err(e) => Err(e),
    }
}

pub fn conn_params(tmp: &ConnectionOptions) -> anyhow::Result<Builder> {
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
            .map_err(|e| ProjectNotFound(e).into())
            .hint(CONNECTION_ARG_HINT)?
            .ok_or_else(|| {
                anyhow::anyhow!("no `edgedb.toml` found \
                    and no connection options are specified")
            })
            .map_err(|e| ProjectNotFound(e).into())
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
    let (host, port) = if tmp.port.is_none() {
        parse_port_env()?
    } else if tmp.host.is_none() {
        // It's okay if we fail to extract host from the EDGEDB_PORT Docker URI
        // because we will try EDGEDB_HOST later (we have tmp.port already)
        match parse_port_env() {
            Ok((host, _)) => (host, tmp.port),
            Err(_) => (None, tmp.port),
        }
    } else {
        (None, tmp.port)
    };
    let host = env_fallback(tmp.host.clone(), "EDGEDB_HOST")?.or(host);
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

pub fn load_tls_options(options: &ConnectionOptions, builder: &mut Builder)
    -> anyhow::Result<()>
{
    if let Some(cert_file) = &options.tls_ca_file {
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
