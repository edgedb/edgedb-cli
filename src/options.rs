use std::env;
use std::path::PathBuf;
use std::time::Duration;

use anymap::AnyMap;
use async_std::task;
use atty;
use clap::{ValueHint};
use colorful::Colorful;
use edgedb_cli_derive::EdbClap;
use edgedb_client::{Builder, get_project_dir};
use edgedb_client::credentials::TlsSecurity;
use edgedb_client::errors::{ClientNoCredentialsError, ErrorKind};
use edgedb_protocol::model;
use fs_err as fs;

use crate::cli::options::CliCommand;
use crate::cli;
use crate::cloud::options::CloudCommand;
use crate::commands::ExitCode;
use crate::commands::parser::Common;
use crate::connect::Connector;
use crate::hint::HintExt;
use crate::markdown;
use crate::portable::project;
use crate::portable;
use crate::portable::options::InstanceName;
use crate::print;
use crate::repl::OutputFormat;
use crate::tty_password;
use crate::watch::options::WatchCommand;

pub mod describe;

const MAX_TERM_WIDTH: usize = 90;
const MIN_TERM_WIDTH: usize = 50;

static CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";

const CONN_OPTIONS_GROUP: &str =
    "CONNECTION OPTIONS (`edgedb --help-connect` to see the full list)";
const CLOUD_OPTIONS_GROUP: &str = "CLOUD OPTIONS";

pub trait PropagateArgs {
    fn propagate_args(&self, dest: &mut AnyMap, matches: &clap::ArgMatches)
        -> Result<(), clap::Error>;
}

#[derive(EdbClap, Clone, Debug)]
#[clap(setting=clap::AppSettings::DeriveDisplayOrder)]
pub struct ConnectionOptions {
    /// Local instance name created with `edgedb instance create` to connect to
    /// (overrides host and port)
    #[clap(short='I', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete instance name
    pub instance: Option<InstanceName>,

    /// DSN for EdgeDB to connect to (overrides all other options
    /// except password)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(conflicts_with_all=&["instance"])]
    pub dsn: Option<String>,

    /// Path to JSON file to read credentials from
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(conflicts_with_all=&["dsn", "instance"])]
    pub credentials_file: Option<PathBuf>,

    /// Host of the EdgeDB instance
    #[clap(short='H', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Hostname)]
    #[clap(hide=true)]
    #[clap(conflicts_with_all=
           &["dsn", "credentials_file", "instance", "unix_path"])]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[clap(short='P', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    #[clap(conflicts_with_all=&["dsn", "credentials_file", "instance"])]
    pub port: Option<u16>,

    /// A path to a unix socket for the EdgeDB connection
    ///
    /// When the supplied path is a directory, the actual path will be
    /// computed using the `--port` and `--admin` parameters.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::AnyPath)]
    #[clap(hide=true)]
    #[clap(conflicts_with_all=
           &["dsn", "credentials_file", "instance", "host"])]
    pub unix_path: Option<PathBuf>,

    /// User name of the EdgeDB user
    #[clap(short='u', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short='d', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete database
    #[clap(hide=true)]
    pub database: Option<String>,

    /// Ask for password on the terminal (TTY)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub password: bool,

    /// Don't ask for password
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub no_password: bool,

    /// Read the password from stdin rather than TTY (useful for scripts)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub password_from_stdin: bool,

    /// Secret key to authenticate with
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub secret_key: Option<String>,

    /// Certificate to match server against
    ///
    /// This might either be full self-signed server certificate or certificate
    /// authority (CA) certificate that server certificate is signed with.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub tls_ca_file: Option<PathBuf>,


    /// Verify hostname of the server using provided certificate
    ///
    /// It's useful when certificate authority (CA) is used for handling
    /// certificate and usually not used for self-signed certificates.
    ///
    /// By default it's enabled when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file)
    #[clap(long, hide=true)]
    #[clap(conflicts_with_all=&["no_tls_verify_hostname"])]
    pub tls_verify_hostname: bool, // deprecated for tls_security

    /// Do not verify hostname of the server
    ///
    /// This allows using any certificate for any hostname. However,
    /// certificate must be present and match certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    #[clap(long, hide=true)]
    #[clap(conflicts_with_all=&["tls_verify_hostname"])]
    pub no_tls_verify_hostname: bool, // deprecated for tls_security

    /// Specify the client-side TLS security mode.
    ///
    /// `insecure`:
    /// Do not verify server certificate at all, only use encryption.
    ///
    /// `no_host_verification`:
    /// This allows using any certificate for any hostname. However,
    /// certificate must be present and match certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    ///
    /// `strict`:
    /// Verify the server certificate and check the hostname.
    /// It's useful when certificate authority (CA) is used for handling
    /// certificate and usually not used for self-signed certificates.
    ///
    /// `default`:
    /// By default it's "strict" when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file), or
    /// "no_host_verification" otherwise.
    #[clap(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_name="insecure | no_host_verification | strict | default")]
    tls_security: Option<String>,

    /// In case EdgeDB connection can't be established, retry up to
    /// WAIT_TIME (e.g. '30s').
    #[clap(long, name="WAIT_TIME", help_heading=Some(CONN_OPTIONS_GROUP),
                parse(try_from_str=parse_duration))]
    #[clap(hide=true)]
    pub wait_until_available: Option<Duration>,

    /// Connect to a passwordless unix socket with superuser
    /// privileges by default.
    #[clap(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    pub admin: bool,

    /// In case EdgeDB doesn't respond for a TIMEOUT, fail
    /// (or retry if `--wait-until-available` is also specified). Default '10s'.
    #[clap(long, name="TIMEOUT", help_heading=Some(CONN_OPTIONS_GROUP),
           parse(try_from_str=parse_duration))]
    #[clap(hide=true)]
    pub connect_timeout: Option<Duration>,
}

#[derive(EdbClap, Clone, Debug)]
#[clap(setting=clap::AppSettings::DeriveDisplayOrder)]
pub struct CloudOptions {
    /// Specify the EdgeDB Cloud API endpoint, default to the current logged-in
    /// server, or https://api.g.aws.edgedb.cloud if unauthorized
    #[clap(long, name="URL", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub cloud_api_endpoint: Option<String>,

    /// Specify the EdgeDB Cloud API access token to use, instead of loading
    /// the access token from the remembered authentication.
    #[clap(long, name="TOKEN", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub cloud_access_token: Option<String>,
}

/// Use the `edgedb` command-line tool to spin up local instances,
/// manage EdgeDB projects, create and apply migrations, and more.
///
/// Running `edgedb` without a subcommand opens an interactive shell.
#[derive(EdbClap, Debug)]
#[edb(main)]
#[clap(disable_version_flag=true)]
pub struct RawOptions {
    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"), clap(hide=true))]
    pub debug_print_frames: bool,

    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"), clap(hide=true))]
    pub debug_print_descriptors: bool,

    #[clap(long)]
    #[cfg_attr(not(feature="dev_mode"), clap(hide=true))]
    pub debug_print_codecs: bool,

    /// Print all available connection options
    /// for the interactive shell and subcommands
    #[clap(long)]
    pub help_connect: bool,

    /// Tab-separated output of the queries
    #[clap(short='t', long, overrides_with="json", hide=true)]
    pub tab_separated: bool,
    /// JSON output for the queries (single JSON list per query)
    #[clap(short='j', long, overrides_with="tab_separated", hide=true)]
    pub json: bool,
    /// Execute a query instead of starting REPL
    #[clap(short='c', hide=true)]
    pub query: Option<String>,

    /// Show command-line tool version
    #[clap(short='V', long="version")]
    pub print_version: bool,

    // (deprecated in favor of "no_cli_update_check")
    #[clap(long, hide=true)]
    pub no_version_check: bool,

    /// Disable checking if a new version of CLI is available
    #[clap(long)]
    pub no_cli_update_check: bool,

    #[edb(inheritable)]
    pub conn: ConnectionOptions,

    #[edb(inheritable)]
    pub cloud: CloudOptions,

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
    /// Launch the web UI for the EdgeDB instance
    #[edb(inherit(ConnectionOptions))]
    UI(UI),
    /// Show information about the EdgeDB installation
    Info(Info),
    /// Manage project installation
    #[edb(expand_help)]
    Project(project::ProjectCommand),
    /// Manage local EdgeDB instances
    #[edb(expand_help)]
    Instance(portable::options::ServerInstanceCommand),
    /// Manage local EdgeDB installations
    Server(portable::options::ServerCommand),
    /// Generate shell completions
    #[clap(name="_gen_completions")]
    #[edb(hide=true)]
    _GenCompletions(cli::install::GenCompletions),
    /// Self-installation commands
    #[clap(name="cli")]
    #[edb(expand_help)]
    CliCommand(CliCommand),
    /// Install EdgeDB
    #[clap(name="_self_install")]
    #[edb(hide=true)]
    _SelfInstall(cli::install::CliInstall),
    /// EdgeDB Cloud authentication
    #[edb(inherit(CloudOptions), hide=true)]
    Cloud(CloudCommand),
    /// EdgeDB Cloud authentication
    #[edb(inherit(CloudOptions), hide=true)]
    Watch(WatchCommand),
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

#[derive(EdbClap, Clone, Debug)]
pub struct UI {
    /// Print URL in console instead of opening in the browser
    #[clap(long)]
    pub print_url: bool,

    /// Don't probe the UI endpoint of the server instance
    #[clap(long)]
    pub no_server_check: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Info {
   #[clap(long, possible_values=&[
        "config-dir",
        "cache-dir",
        "data-dir",
        "service-dir",
    ][..])]
    /// Get specific value:
    ///
    /// * `config-dir` -- Base configuration directory
    /// * `cache-dir` -- Base cache directory
    /// * `data-dir` -- Base data directory
    /// * `service-dir` -- Directory where supervisor/startup files are placed
    pub get: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub conn_options: ConnectionOptions,
    pub cloud_options: CloudOptions,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_format: Option<OutputFormat>,
    pub no_cli_update_check: bool,
}

fn parse_duration(value: &str) -> anyhow::Result<Duration> {
    let value = value.parse::<model::Duration>()?;
    match value.is_negative() {
        false => Ok(value.abs_duration()),
        true => anyhow::bail!("negative durations are unsupported"),
    }
}

fn say_option_is_deprecated(option_name: &str, suggestion: &str) {
    let mut error = "warning:".to_string();
    let mut instead = suggestion.to_string();
    if print::use_color() {
        error = format!("{}", error.bold().light_yellow());
        instead = format!("{}", instead.green());
    }
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
        if cmd.hide {
            continue;
        }
        if cmd.expand_help {
            if !empty_line {
                buf.push('\n');
            }
            for subcmd in cdescr.subcommands() {
                let sdescr = subcmd.describe();
                if subcmd.hide {
                    continue;
                }
                writeln!(&mut buf, "    {:padding$} {}",
                    format!("{} {}", cmd.name, subcmd.name),
                    wrap(&markdown::format_title(sdescr.help_title)),
                    padding=padding
                ).unwrap();
            }
            buf.push('\n');
            empty_line = true;
        } else {
            writeln!(&mut buf, "    {:padding$} {}",
                cmd.name, wrap(&markdown::format_title(cdescr.help_title)),
                padding=padding
            ).unwrap();
            empty_line = false;
        }
    }
    buf.truncate(buf.trim_end().len());

    return buf;
}

fn update_main_help(mut app: clap::Command) -> clap::Command {
    if !print::use_color() {
        app = app.color(clap::ColorChoice::Never);
    }
    let sub_cmd = make_subcommand_help::<RawOptions>();
    let mut help = Vec::with_capacity(2048);

    app.write_help(&mut help).unwrap();

    let help = String::from_utf8(help).unwrap();
    let subcmd_index = help.find("SUBCOMMANDS:").unwrap();
    let mut help = help[..subcmd_index].replacen("edgedb", "EdgeDB CLI", 1);
    help.push_str(&sub_cmd);

    help = help.replacen(
        CONN_OPTIONS_GROUP,
        &markdown::format_markdown(CONN_OPTIONS_GROUP).trim(),
        1
    );

    let help = std::str::from_utf8(Vec::leak(help.into())).unwrap();
    return app.override_help(help);
}

fn print_full_connection_options() {
    let app = <ConnectionOptions as clap::CommandFactory>::command();

    let mut new_app = clap::Command::new("edgedb-connect")
                      .setting(clap::AppSettings::DeriveDisplayOrder)
                      .term_width(term_width());
    if !print::use_color() {
        new_app = new_app.color(clap::ColorChoice::Never);
    }

    for arg in app.get_arguments() {
        let arg_name = arg.get_id();
        if arg_name == "help" || arg_name == "version" || arg_name == "admin" {
            continue
        }
        let new_arg = arg.clone().hide(false);
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
        let app = <RawOptions as clap::IntoApp>::command()
                  .name("edgedb")
                  .term_width(term_width());
        let app = update_main_help(app);
        let matches = app.get_matches();
        let tmp: RawOptions = <RawOptions as clap::FromArgMatches>
            ::from_arg_matches(&matches)?;

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
            let mut error = "warning:".to_string();
            if print::use_color() {
                error = format!("{}", error.bold().light_yellow());
            }
            eprintln!("\
                {error} The '--no-version-check' option was renamed.\n\
                \n         \
                    Use '--no-cli-update-check' instead.\
                \n\
            ", error=error);
        }

        Ok(Options {
            conn_options: tmp.conn,
            cloud_options: tmp.cloud,
            interactive,
            subcommand,
            debug_print_frames: tmp.debug_print_frames,
            debug_print_descriptors: tmp.debug_print_descriptors,
            debug_print_codecs: tmp.debug_print_codecs,
            output_format: if tmp.tab_separated {
                Some(OutputFormat::TabSeparated)
            } else if tmp.json {
                Some(OutputFormat::Json)
            } else {
                None
            },
            no_cli_update_check,
        })
    }

    pub fn create_connector(&self) -> anyhow::Result<Connector> {
        Ok(Connector::new(conn_params(&self)))
    }
}

fn set_password(options: &ConnectionOptions, builder: &mut Builder)
    -> anyhow::Result<()>
{
    let password = if options.password_from_stdin {
        tty_password::read_stdin()?
    } else if options.no_password {
        return Ok(());
    } else if options.password {
        let user = builder.get_user();
        tty_password::read(
            format!("Password for '{}': ", user.escape_default()))?
    } else {
        return Ok(())
    };
    builder.password(password);
    Ok(())
}

pub fn conn_params(opts: &Options) -> anyhow::Result<Builder> {
    let tmp = &opts.conn_options;
    let mut bld = Builder::uninitialized();
    if let Some(path) = &tmp.unix_path {
        bld.unix_path(path, tmp.port, tmp.admin);
        bld.read_extra_env_vars()?;
    } else if tmp.host.is_some() || tmp.port.is_some() {
        match &tmp.host {
            Some(host) if host.contains("/") => {
                log::warn!("Deprecated: `--host` that contains slash is path \
                    to a unix socket. Use TCP connection if possible \
                    otherwise use `--unix-path`.");
                bld.unix_path(host, tmp.port, tmp.admin);
            }
            _ => {
                bld.host_port(tmp.host.clone(), tmp.port);
            }
        }
        bld.read_extra_env_vars()?;
    } else if let Some(dsn) = &tmp.dsn {
        task::block_on(bld.read_dsn(dsn))?;
        bld.read_extra_env_vars()?;
    } else if let Some(instance) = &tmp.instance {
        match instance {
            InstanceName::Cloud { org_slug, name } => {
                let client = crate::cloud::client::CloudClient::new(&opts.cloud_options)?;
                client.ensure_authenticated()?;
                bld.host_port(Some(client.get_cloud_host(org_slug, name)), None);
                bld.secret_key(client.access_token.unwrap());
            }
            InstanceName::Local(instance) => {
                task::block_on(bld.read_instance(instance))?;
            }
        }
        bld.read_extra_env_vars()?;
    } else if let Some(file_path) = &tmp.credentials_file {
        task::block_on(bld.read_credentials(file_path))?;
        bld.read_extra_env_vars()?;
    } else {
        bld = task::block_on(Builder::from_env())?;
    };
    if tmp.admin {
        bld.admin()?;
    }
    if let Some(user) = &tmp.user {
        bld.user(user);
    }
    if let Some(database) = &tmp.database {
        bld.database(database);
    }
    if let Some(val) = tmp.wait_until_available {
        bld.wait_until_available(val);
    }
    if let Some(val) = tmp.connect_timeout {
        bld.connect_timeout(val);
    }
    if let Some(val) = &tmp.secret_key {
        bld.secret_key(val);
    }
    set_password(tmp, &mut bld)?;
    load_tls_options(tmp, &mut bld)?;
    if !bld.is_initialized() {
        let project_dir = task::block_on(get_project_dir(None, true))?;
        if project_dir.is_some() {
            return Err(anyhow::anyhow!(ClientNoCredentialsError::with_message(
                "project is not initialized \
                 and no connection options are specified"
            ))).hint(CONNECTION_ARG_HINT)?;
        } else {
            return Err(anyhow::anyhow!(ClientNoCredentialsError::with_message(
                "no `edgedb.toml` found and no connection options are specified"
            ))).hint(CONNECTION_ARG_HINT)?;
        }
    }
    Ok(bld)
}

pub fn load_tls_options(options: &ConnectionOptions, builder: &mut Builder)
    -> anyhow::Result<()>
{
    if let Some(cert_file) = &options.tls_ca_file {
        let data = fs::read_to_string(cert_file)?;
        builder.pem_certificates(&data)?;
    }
    let mut security = match options.tls_security.as_deref() {
        None => None,
        Some("insecure") => Some(TlsSecurity::Insecure),
        Some("no_host_verification") => Some(TlsSecurity::NoHostVerification),
        Some("strict") => Some(TlsSecurity::Strict),
        Some("default") => Some(TlsSecurity::Default),
        Some(_) => anyhow::bail!(
            "Unsupported TLS security, options: \
             `default`, `strict`, `no_host_verification`, `insecure`"
        ),
    };
    if options.no_tls_verify_hostname {
        if let Some(s) = security {
            if s != TlsSecurity::NoHostVerification {
                anyhow::bail!(
                    "Cannot set --no-tls-verify-hostname while \
                     --tls-security is also set"
                );
            }
        } else {
            security = Some(TlsSecurity::NoHostVerification);
        }
    }
    if options.tls_verify_hostname {
        if let Some(s) = security {
            if s != TlsSecurity::Strict {
                anyhow::bail!(
                    "Cannot set --tls-verify-hostname while \
                     --tls-security is also set"
                );
            }
        } else {
            security = Some(TlsSecurity::Strict);
        }
    }
    if let Some(s) = security {
        builder.tls_security(s);
    }
    Ok(())
}
