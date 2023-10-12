use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::time::Duration;
use std::io::stdin;

use clap::{ValueHint};
use colorful::Colorful;
use edgedb_cli_derive::EdbClap;
use edgedb_errors::{ClientNoCredentialsError, ResultExt};
use edgedb_protocol::model;
use edgedb_tokio::credentials::TlsSecurity;
use edgedb_tokio::{Builder, Config, get_project_dir};
use is_terminal::IsTerminal;
use tokio::task::spawn_blocking as unblock;

use crate::cli::options::CliCommand;
use crate::cli;
use crate::cloud::options::CloudCommand;
use crate::commands::ExitCode;
use crate::commands::parser::Common;
use crate::connect::Connector;
use crate::hint::HintExt;
use crate::markdown;
use crate::portable::local::{instance_data_dir, runstate_dir};
use crate::portable::options::InstanceName;
use crate::portable::project;
use crate::portable;
use crate::print;
use crate::repl::OutputFormat;
use crate::tty_password;
use crate::watch::options::WatchCommand;

pub mod describe;

const MAX_TERM_WIDTH: usize = 90;
const MIN_TERM_WIDTH: usize = 50;

const CONN_OPTIONS_GROUP: &str =
    "CONNECTION OPTIONS (`edgedb --help-connect` to see full list)";
const CLOUD_OPTIONS_GROUP: &str = "CLOUD OPTIONS";
const CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";

pub struct SharedGroups(HashMap<TypeId, Box<dyn Any>>);

pub trait PropagateArgs {
    fn propagate_args(&self, dest: &mut SharedGroups,
                      matches: &clap::ArgMatches)
        -> Result<(), clap::Error>;
}

#[derive(EdbClap, Clone, Debug)]
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
    #[clap(hide=true)]
    pub credentials_file: Option<PathBuf>,

    /// EdgeDB instance host
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

    /// A path to a Unix socket for EdgeDB connection
    ///
    /// When the supplied path is a directory, the actual path will be
    /// computed using the `--port` and `--admin` parameters.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::AnyPath)]
    #[clap(hide=true)]
    #[clap(conflicts_with_all=
           &["dsn", "credentials_file", "instance", "host"])]
    pub unix_path: Option<PathBuf>,

    /// EdgeDB user name
    #[clap(short='u', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub user: Option<String>,

    /// Database name to connect to
    #[clap(short='d', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_hint=ValueHint::Other)]  // TODO complete database
    #[clap(hide=true)]
    pub database: Option<String>,

    /// Ask for password on terminal (TTY)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub password: bool,

    /// Don't ask for password
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub no_password: bool,

    /// Read password from stdin rather than TTY (useful for scripts)
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub password_from_stdin: bool,

    /// Secret key to authenticate with
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub secret_key: Option<String>,

    /// Certificate to match server against
    ///
    /// Might either be a full self-signed server certificate or certificate
    /// authority (CA) certificate that the server certificate is signed with.
    #[clap(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub tls_ca_file: Option<PathBuf>,

    /// Verify server hostname using provided certificate.
    ///
    /// Useful when certificate authority (CA) is used for certificate
    /// handling and usually not used for self-signed certificates.
    ///
    /// Enabled by default when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file)
    #[clap(long, hide=true)]
    #[clap(conflicts_with_all=&["no_tls_verify_hostname"])]
    pub tls_verify_hostname: bool, // deprecated for tls_security

    /// Do not verify server hostname
    ///
    /// This allows using any certificate for any hostname. However,
    /// a certificate must be present and matching certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    #[clap(long, hide=true)]
    #[clap(conflicts_with_all=&["tls_verify_hostname"])]
    pub no_tls_verify_hostname: bool, // deprecated for tls_security

    /// Specifications for client-side TLS security mode:
    ///
    /// `insecure`:
    /// Do not verify server certificate at all, only use encryption.
    ///
    /// `no_host_verification`:
    /// This allows using any certificate for any hostname. However,
    /// a certificate must be present and matching certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    ///
    /// `strict`:
    /// Verify server certificate and check hostname.
    /// Useful when certificate authority (CA) is used for certificate
    /// handling and usually not used for self-signed certificates.
    ///
    /// `default`:
    /// Defaults to "strict" when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file); otherwise
    /// to "no_host_verification".
    #[clap(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[clap(value_name="insecure | no_host_verification | strict | default")]
    tls_security: Option<String>,

    /// Retry up to WAIT_TIME (e.g. '30s') in case EdgeDB connection
    /// cannot be established.
    #[clap(long, name="WAIT_TIME", help_heading=Some(CONN_OPTIONS_GROUP),
                parse(try_from_str=parse_duration))]
    #[clap(hide=true)]
    pub wait_until_available: Option<Duration>,

    /// Connect to a passwordless Unix socket with superuser
    /// privileges by default.
    #[clap(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    pub admin: bool,

    /// Fail when no response from EdgeDB for TIMEOUT (default '10s');
    /// alternatively will retry if `--wait-until-available` is also specified.
    #[clap(long, name="TIMEOUT", help_heading=Some(CONN_OPTIONS_GROUP),
           parse(try_from_str=parse_duration))]
    #[clap(hide=true)]
    pub connect_timeout: Option<Duration>,
}

#[derive(EdbClap, Clone, Debug)]
pub struct CloudOptions {
    /// Specify the EdgeDB Cloud API endpoint. Defaults to the current logged-in
    /// server, or <https://api.g.aws.edgedb.cloud> if unauthorized
    #[clap(long, name="URL", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub cloud_api_endpoint: Option<String>,

    /// Specify EdgeDB Cloud API secret key to use instead of loading
    /// key from a remembered authentication.
    #[clap(long, name="SECRET_KEY", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub cloud_secret_key: Option<String>,

    /// Specify authenticated EdgeDB Cloud profile. Defaults to "default".
    #[clap(long, name="PROFILE", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[clap(hide=true)]
    pub cloud_profile: Option<String>,
}

/// Use the `edgedb` command-line tool to spin up local instances,
/// manage EdgeDB projects, create and apply migrations, and more.
///
/// Running `edgedb` without a subcommand opens an interactive shell
/// for the instance in your directory. If you have no existing instance,
/// type `edgedb project init` to create one.
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

    #[cfg(feature="portable_tests")]
    #[clap(long)]
    pub test_output_conn_params: bool,

    /// Print all available connection options
    /// for interactive shell along with subcommands
    #[clap(long)]
    pub help_connect: bool,

    /// Tab-separated output for queries
    #[clap(short='t', long, overrides_with="json", hide=true)]
    pub tab_separated: bool,
    /// JSON output for queries (single JSON list per query)
    #[clap(short='j', long, overrides_with="tab_separated", hide=true)]
    pub json: bool,
    /// Execute a query instead of starting REPL
    #[clap(short='c', hide=true)]
    pub query: Option<String>,

    /// Show command-line tool version
    #[clap(short='V', long="version")]
    pub print_version: bool,

    // Deprecated: use "no_cli_update_check" instead
    #[clap(long, hide=true)]
    pub no_version_check: bool,

    /// Disable check for new available CLI version
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
    /// Execute EdgeQL query in quotes (e.g. `"select 9;"`)
    #[edb(inherit(ConnectionOptions))]
    Query(Query),
    /// Launch EdgeDB instance in browser web UI
    #[edb(inherit(ConnectionOptions))]
    #[edb(inherit(CloudOptions))]
    UI(UI),
    /// Show paths for EdgeDB installation
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
    /// Start a long-running process that watches for changes in schema files in
    /// a project's ``dbschema`` directory, applying them in real time.
    #[edb(inherit(CloudOptions))]
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

    /// Do not probe the UI endpoint of the server instance
    #[clap(long)]
    pub no_server_check: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct Info {
   #[clap(long, value_parser=[
        "config-dir",
        "cache-dir",
        "data-dir",
        "service-dir",
    ])]
    /// Get specific value:
    ///
    /// * `config-dir` -- Base configuration directory
    /// * `cache-dir` -- Base cache directory
    /// * `data-dir` -- Base data directory (except on Windows)
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
    #[cfg(feature="portable_tests")]
    pub test_output_conn_params: bool,
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

    println!("CONNECTION OPTIONS (full list):\n");
    println!("{}", help);
}

fn term_width() -> usize {
    use std::cmp;

    // clap::Command::max_term_width() works poorly in conjunction
    // with  clap::Command::term_width(); it appears that one call
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
        let app = <RawOptions as clap::CommandFactory>::command()
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
            && stdin().is_terminal();

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
            #[cfg(feature="portable_tests")]
            test_output_conn_params: tmp.test_output_conn_params,
        })
    }

    pub async fn create_connector(&self) -> anyhow::Result<Connector> {
        let mut builder = prepare_conn_params(&self)?;
        if self.conn_options.password_from_stdin || self.conn_options.password {
            // Temporary set an empty password. It will be overriden by
            // `config.with_password()` but we need it here so that
            // `edgedb://?password_env=NON_EXISTING` does not read the
            // environemnt variable
            builder.password("");
        }
        match builder.build_env().await {
            Ok(config) => {
                let mut cfg = with_password(&self.conn_options, config).await?;
                match (cfg.admin(), cfg.port(), cfg.local_instance_name()) {
                    (false, _, _) => {}
                    (true, None, _) => {}
                    (true, Some(port), Some(name)) => {
                        if !instance_data_dir(name)?.exists() {
                            anyhow::bail!("The --admin option requires \
                                           --unix-path or local instance name");
                        }
                        let sock = runstate_dir(name)?
                            .join(format!(".s.EDGEDB.admin.{}", port));
                        cfg = cfg.with_unix_path(&sock);
                    }
                    (true, Some(_), None) => {
                        anyhow::bail!("The --admin option requires \
                                       --unix-path or local instance name");
                    }
                }
                Ok(Connector::new(Ok(cfg)))
            }
            Err(e) => {
                let (_, cfg, _) = builder.build_no_fail().await;
                // ask password anyways, so input that fed as a password
                // never goes to anywhere else
                with_password(&self.conn_options, cfg).await?;

                if e.is::<ClientNoCredentialsError>() {
                    let project_dir = get_project_dir(None, true).await?;
                    let message = if project_dir.is_some() {
                        "project is not initialized and no connection options \
                            are specified"
                    } else {
                        "no `edgedb.toml` found and no connection options \
                            are specified"
                    };
                    Ok(Connector::new(
                        Err(anyhow::anyhow!(message))
                        .hint(CONNECTION_ARG_HINT)
                        .map_err(Into::into)
                    ))
                } else {
                    Ok(Connector::new(Err(e.into())))
                }
            }
        }
    }

    #[tokio::main]
    pub async fn block_on_create_connector(&self) -> anyhow::Result<Connector>
    {
        self.create_connector().await
    }
}

async fn with_password(options: &ConnectionOptions, config: Config)
    -> anyhow::Result<Config>
{
    if options.password_from_stdin {
        let password = unblock(|| tty_password::read_stdin()).await??;
        Ok(config.with_password(&password))
    } else if options.no_password {
        Ok(config)
    } else if options.password {
        let user = config.user().to_owned();
        let password = unblock(move || {
            tty_password::read(
                format!("Password for '{}': ", user.escape_default()))
        }).await??;
        Ok(config.with_password(&password))
    } else {
        Ok(config)
    }
}

pub fn prepare_conn_params(opts: &Options) -> anyhow::Result<Builder> {
    let tmp = &opts.conn_options;
    let mut bld = Builder::new();
    if let Some(path) = &tmp.unix_path {
        bld.unix_path(path);
    }
    if let Some(host) = &tmp.host {
        if host.contains('/') {
            log::warn!("Deprecated: `--host` containing a slash is \
                a path to a unix socket. Use TCP connection if possible, \
                otherwise use `--unix-path`.");
            bld.unix_path(host);
        } else {
            bld.host(host)?;
        }
    }
    if let Some(port) = tmp.port {
        bld.port(port)?;
    }
    if let Some(dsn) = &tmp.dsn {
        bld.dsn(dsn).context("invalid DSN")?;
    }
    if let Some(instance) = &tmp.instance {
        bld.instance(&instance.to_string())?;
    }
    if let Some(secret_key) = &tmp.secret_key {
        bld.secret_key(secret_key);
    }
    if let Some(file_path) = &tmp.credentials_file {
        bld.credentials_file(file_path);
    }
    if tmp.admin {
        bld.admin(true);
    }
    if let Some(user) = &tmp.user {
        bld.user(user)?;
    }
    if let Some(database) = &tmp.database {
        bld.database(database)?;
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
    load_tls_options(tmp, &mut bld)?;
    Ok(bld)
}

pub fn load_tls_options(options: &ConnectionOptions, builder: &mut Builder)
    -> anyhow::Result<()>
{
    if let Some(cert_file) = &options.tls_ca_file {
        builder.tls_ca_file(&cert_file);
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

impl SharedGroups {
    pub fn new() -> SharedGroups {
        SharedGroups(HashMap::new())
    }
    pub fn insert<T: Any>(&mut self, value: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(value));
    }
    pub fn remove<T: Any>(&mut self) -> Option<T> {
        self.0.remove(&TypeId::of::<T>()).map(|v| *v.downcast().unwrap())
    }
    pub fn get_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>()).and_then(|v| v.downcast_mut())
    }
}
