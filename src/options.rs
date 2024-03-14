use std::env;
use std::path::PathBuf;
use std::time::Duration;
use std::io::stdin;

use colorful::Colorful;
use edgedb_errors::{ClientNoCredentialsError, ResultExt};
use edgedb_protocol::model;
use edgedb_tokio::credentials::TlsSecurity;
use edgedb_tokio::{Builder, Config, get_project_dir};
use is_terminal::IsTerminal;
use tokio::task::spawn_blocking as unblock;

use edgedb_cli_derive::IntoArgs;

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
use crate::branch::option::BranchCommand;

const MAX_TERM_WIDTH: usize = 100;
const MIN_TERM_WIDTH: usize = 50;

const CONN_OPTIONS_GROUP: &str =
    "Connection Options (edgedb --help-connect to see full list)";
const CLOUD_OPTIONS_GROUP: &str = "Cloud Connection Options";
const CONNECTION_ARG_HINT: &str = "\
    Run `edgedb project init` or use any of `-H`, `-P`, `-I` arguments \
    to specify connection parameters. See `--help` for details";

#[derive(clap::Args, Clone, Debug)]
#[group(id = "connopts")]
pub struct ConnectionOptions {
    /// Instance name (use `edgedb instance list` to list local, remote and
    /// Cloud instances available to you).
    #[arg(short='I', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_hint=clap::ValueHint::Other)]  // TODO complete instance name
    #[arg(global=true)]
    pub instance: Option<InstanceName>,

    /// DSN for EdgeDB to connect to (overrides all other options
    /// except password)
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(conflicts_with_all=&["instance"])]
    #[arg(global=true)]
    pub dsn: Option<String>,

    /// Path to JSON file to read credentials from
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(conflicts_with_all=&["dsn", "instance"])]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub credentials_file: Option<PathBuf>,

    /// EdgeDB instance host
    #[arg(short='H', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_hint=clap::ValueHint::Hostname)]
    #[arg(hide=true)]
    #[arg(global=true)]
    #[arg(conflicts_with_all=
          &["dsn", "credentials_file", "instance", "unix_path"])]
    pub host: Option<String>,

    /// Port to connect to EdgeDB
    #[arg(short='P', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    #[arg(conflicts_with_all=&["dsn", "credentials_file", "instance"])]
    pub port: Option<u16>,

    /// A path to a Unix socket for EdgeDB connection
    ///
    /// When the supplied path is a directory, the actual path will be
    /// computed using the `--port` and `--admin` parameters.
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_hint=clap::ValueHint::AnyPath)]
    #[arg(hide=true)]
    #[arg(global=true)]
    #[arg(conflicts_with_all=
          &["dsn", "credentials_file", "instance", "host"])]
    pub unix_path: Option<PathBuf>,

    /// EdgeDB user name
    #[arg(short='u', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub user: Option<String>,

    /// Database name to connect to
    #[arg(short='d', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_hint=clap::ValueHint::Other)]  // TODO complete database
    #[arg(hide=true)]
    #[arg(global=true)]
    pub database: Option<String>,

    /// Branch to connect with
    #[arg(short='b', long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_hint=clap::ValueHint::Other)]  // TODO complete database
    #[arg(hide=true)]
    #[arg(global=true)]
    pub branch: Option<String>,

    /// Ask for password on terminal (TTY)
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub password: bool,

    /// Don't ask for password
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub no_password: bool,

    /// Read password from stdin rather than TTY (useful for scripts)
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub password_from_stdin: bool,

    /// Secret key to authenticate with
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub secret_key: Option<String>,

    /// Certificate to match server against
    ///
    /// Might either be a full self-signed server certificate or certificate
    /// authority (CA) certificate that the server certificate is signed with.
    #[arg(long, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub tls_ca_file: Option<PathBuf>,

    /// Verify server hostname using provided certificate.
    ///
    /// Useful when certificate authority (CA) is used for certificate
    /// handling and usually not used for self-signed certificates.
    ///
    /// Enabled by default when no specific certificate is present
    /// (via `--tls-ca-file` or in credentials JSON file)
    #[arg(long, hide=true)]
    #[arg(conflicts_with_all=&["no_tls_verify_hostname"])]
    #[arg(global=true)]
    pub tls_verify_hostname: bool, // deprecated for tls_security

    /// Do not verify server hostname
    ///
    /// This allows using any certificate for any hostname. However,
    /// a certificate must be present and matching certificate specified with
    /// `--tls-ca-file` or credentials file or signed by one of the root
    /// certificate authorities.
    #[arg(long, hide=true)]
    #[arg(conflicts_with_all=&["tls_verify_hostname"])]
    #[arg(global=true)]
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
    #[arg(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(value_name="insecure | no_host_verification | strict | default")]
    #[arg(global=true)]
    tls_security: Option<String>,

    /// Override server name used for TLS connections and certificate
    /// verification.
    ///
    /// Useful when the server hostname cannot be used as it
    /// does not resolve, or resolves to a wrong IP address,
    /// and a different name or IP address is used in `--host`.
    #[arg(long)]
    #[arg(global=true)]
    pub tls_server_name: Option<String>,

    /// Retry up to WAIT_TIME (e.g. '30s') in case EdgeDB connection
    /// cannot be established.
    #[arg(
        long,
        value_name="WAIT_TIME",
        help_heading=Some(CONN_OPTIONS_GROUP),
        value_parser=parse_duration,
    )]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub wait_until_available: Option<Duration>,

    /// Connect to a passwordless Unix socket with superuser
    /// privileges by default.
    #[arg(long, hide=true, help_heading=Some(CONN_OPTIONS_GROUP))]
    #[arg(global=true)]
    pub admin: bool,

    /// Fail when no response from EdgeDB for TIMEOUT (default '10s');
    /// alternatively will retry if `--wait-until-available` is also specified.
    #[arg(
        long,
        value_name="TIMEOUT",
        help_heading=Some(CONN_OPTIONS_GROUP),
        value_parser=parse_duration,
    )]
    #[arg(hide=true)]
    #[arg(global=true)]
    pub connect_timeout: Option<Duration>,
}

impl ConnectionOptions {
    pub(crate) fn get_branch(&self) -> Option<&String> {
        self.database.as_ref().or(self.branch.as_ref())
    }

    pub(crate) fn validate(&self) {
        if self.database.is_some() {
            print::warn("database connection argument is deprecated in favor of 'branch'");
        }
    }
}

#[derive(clap::Parser, Debug)]
#[command(disable_version_flag=true)]
pub struct HelpConnect {
    #[command(flatten)]
    pub conn: ConnectionOptions,
}

#[derive(clap::Args, IntoArgs, Clone, Debug)]
#[group(id = "cloudopts")]
pub struct CloudOptions {
    /// Specify the EdgeDB Cloud API endpoint. Defaults to the current logged-in
    /// server, or <https://api.g.aws.edgedb.cloud> if unauthorized
    #[arg(long, value_name="URL", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[arg(global=true)]
    pub cloud_api_endpoint: Option<String>,

    /// Specify EdgeDB Cloud API secret key to use instead of loading
    /// key from a remembered authentication.
    #[arg(long, value_name="SECRET_KEY", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[arg(global=true)]
    pub cloud_secret_key: Option<String>,

    /// Specify authenticated EdgeDB Cloud profile. Defaults to "default".
    #[arg(long, value_name="PROFILE", help_heading=Some(CLOUD_OPTIONS_GROUP))]
    #[arg(global=true)]
    pub cloud_profile: Option<String>,
}

/// Use the `edgedb` command-line tool to spin up local instances,
/// manage EdgeDB projects, create and apply migrations, and more.
///
/// Running `edgedb` without a subcommand opens an interactive shell
/// for the instance in your directory. If you have no existing instance,
/// type `edgedb project init` to create one.
#[derive(clap::Parser, Debug)]
#[command(disable_version_flag=true)]
pub struct RawOptions {
    #[arg(long)]
    #[cfg_attr(not(feature="dev_mode"), arg(hide=true))]
    pub debug_print_frames: bool,

    #[arg(long)]
    #[cfg_attr(not(feature="dev_mode"), arg(hide=true))]
    pub debug_print_descriptors: bool,

    #[arg(long)]
    #[cfg_attr(not(feature="dev_mode"), arg(hide=true))]
    pub debug_print_codecs: bool,

    #[arg(long, hide=true)]
    pub test_output_conn_params: bool,

    /// Print all available connection options
    /// for interactive shell along with subcommands
    #[arg(long)]
    pub help_connect: bool,

    /// Tab-separated output for queries
    #[arg(short='t', long, overrides_with="json", hide=true)]
    pub tab_separated: bool,
    /// JSON output for queries (single JSON list per query)
    #[arg(short='j', long, overrides_with="tab_separated", hide=true)]
    pub json: bool,
    /// Execute a query instead of starting REPL
    #[arg(short='c', hide=true)]
    pub query: Option<String>,

    /// Show command-line tool version
    #[arg(short='V', long="version")]
    pub print_version: bool,

    // Deprecated: use "no_cli_update_check" instead
    #[arg(long, hide=true)]
    pub no_version_check: bool,

    /// Disable check for new available CLI version
    #[arg(long)]
    pub no_cli_update_check: bool,

    #[command(flatten)]
    pub conn: ConnectionOptions,

    #[command(flatten)]
    pub cloud: CloudOptions,
}

#[derive(clap::Args, Debug)]
pub struct SubcommandOption {
    #[command(subcommand)]
    pub subcommand: Option<Command>,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    #[command(flatten)]
    Common(Common),
    /// Execute EdgeQL query in quotes (e.g. `"select 9;"`)
    Query(Query),
    /// Launch EdgeDB instance in browser web UI
    UI(UI),
    /// Show paths for EdgeDB installation
    Info(Info),
    /// Manage project installation
    Project(project::ProjectCommand),
    /// Manage local EdgeDB instances
    Instance(portable::options::ServerInstanceCommand),
    /// Manage local EdgeDB installations
    Server(portable::options::ServerCommand),
    /// Generate shell completions
    #[command(name="_gen_completions")]
    #[command(hide=true)]
    _GenCompletions(cli::install::GenCompletions),
    /// Self-installation commands
    #[command(name="cli")]
    CliCommand(CliCommand),
    /// Install EdgeDB
    #[command(name="_self_install")]
    #[command(hide=true)]
    _SelfInstall(cli::install::CliInstall),
    /// EdgeDB Cloud authentication
    Cloud(CloudCommand),
    /// Start a long-running process that watches for changes in schema files in
    /// a project's ``dbschema`` directory, applying them in real time.
    Watch(WatchCommand),
    /// Manage branches
    Branch(BranchCommand),
}

#[derive(clap::Args, Clone, Debug)]
pub struct Query {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    /// Output format: `json`, `json-pretty`, `json-lines`, `tab-separated`.
    /// Default is `json-pretty`.
    // todo: can't use `arg(default='json-pretty')` just yet, as we
    // need to see if the user did actually specify some output
    // format or not. We need that to support the now deprecated
    // --json and --tab-separated top-level options.
    #[arg(short='F', long)]
    pub output_format: Option<OutputFormat>,

    /// Filename to execute queries from.
    /// Pass `--file -` to execute queries from stdin.
    #[arg(short='f', long)]
    pub file: Option<String>,

    pub queries: Option<Vec<String>>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct UI {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    /// Print URL in console instead of opening in the browser
    #[arg(long)]
    pub print_url: bool,

    /// Do not probe the UI endpoint of the server instance
    #[arg(long)]
    pub no_server_check: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct Info {
   #[arg(long, value_parser=[
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
    pub app: clap::Command,
    pub conn_options: ConnectionOptions,
    pub cloud_options: CloudOptions,
    pub subcommand: Option<Command>,
    pub interactive: bool,
    pub debug_print_frames: bool,
    pub debug_print_descriptors: bool,
    pub debug_print_codecs: bool,
    pub output_format: Option<OutputFormat>,
    pub no_cli_update_check: bool,
    pub test_output_conn_params: bool,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("error: {}", msg)]
pub struct UsageError {
    kind: clap::error::ErrorKind,
    msg: String,
}

impl UsageError {
    pub fn new(kind: clap::error::ErrorKind, msg: impl std::fmt::Display) -> Self {
        UsageError{kind, msg: msg.to_string()}
    }
    pub fn exit(&self) -> ! {
        clap::Error::raw(self.kind, &self.msg).exit()
    }
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

fn make_subcommand_help(parent: &clap::Command) -> String {
    use std::fmt::Write;

    let width = term_width();

    // When the terminal is wider than 82 characters clap aligns
    // the flags description text to the right of the flag name,
    // when it is narrower than 82, the description goes below
    // the option name.  We want to align the subcommand description
    // with the option description, hence there's some hand-tuning
    // of the padding here.
    let padding: usize = if width > 82 { 26 } else { 24 };

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
                format!("  {:padding$} {}", " ", line, padding=padding)
            );
        }

        new_lines.join("\n")
    };

    let mut buf = String::with_capacity(4096);

    write!(
        &mut buf,
        color_print::cstr!("<bold><underline>Commands</underline></bold>:\n"),
    ).unwrap();
    let mut empty_line = true;

    for cmd in parent.get_subcommands() {
        if cmd.is_hide_set() {
            continue;
        }
        if cmd.get_version() == Some("help_expand") {
            if !empty_line {
                buf.push('\n');
            }
            for subcmd in cmd.get_subcommands() {
                if subcmd.is_hide_set() {
                    continue;
                }
                writeln!(&mut buf, "  {} {}",
                    color_print::cformat!(
                        "<bold>{:padding$}</bold>",
                        format!("{} {}", cmd.get_name(), subcmd.get_name()),
                        padding=padding,
                    ),
                    wrap(&markdown::format_title(
                        &subcmd
                            .get_about()
                            .or_else(|| subcmd.get_long_about())
                            .unwrap_or_default()
                            .to_string()
                    )),
                ).unwrap();
            }
            buf.push('\n');
            empty_line = true;
        } else {
            writeln!(&mut buf, "  {} {}",
                color_print::cformat!(
                    "<bold>{:padding$}</bold>",
                    cmd.get_name(),
                    padding=padding,
                ),
                wrap(&markdown::format_title(
                    &cmd
                        .get_about()
                        .or_else(|| cmd.get_long_about())
                        .unwrap_or_default()
                        .to_string()
                )),
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
    let sub_cmd = make_subcommand_help(&app);

    let help = format!("{}", app.render_help().ansi()).to_string();
    let subcmd_index = help.find("Commands:").unwrap();
    let opt_index = help.find("Options:").unwrap();

    let help = vec![
        &help[..subcmd_index],
        &sub_cmd,
        &color_print::cformat!("\n\n<bold><underline>Options:</underline></bold>"),
        &help[(opt_index + 8)..]
    ].join("");

    let help = std::str::from_utf8(Vec::leak(help.into())).unwrap();
    return app.override_help(help);
}

fn print_full_connection_options() {
    let app = <HelpConnect as clap::CommandFactory>::command();

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

    // "Long help" has more whitespace and is much more readable
    // for the many options we have in the connection group.
    let help = format!("{}", new_app.render_long_help().ansi());
    let subcmd_index = help.find(CONN_OPTIONS_GROUP).unwrap();
    let slice_from = subcmd_index + CONN_OPTIONS_GROUP.len() + 1;
    let help = &help[slice_from..];

    color_print::cprintln!("<bold><underline>Connection Options (full list):</underline></bold>");
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
    pub fn error(&self, kind: clap::error::ErrorKind, msg: impl std::fmt::Display) -> UsageError {
        UsageError::new(kind, msg)
    }

    pub fn command() -> clap::Command {
        // Connection/Cloud options apply *both* to the
        // root command when ran without arguments (i.e. REPL mode)
        // and to many, but, crucially, not ALL subcommands, so
        // we cannot simply make ConnectionOptions and CloudOptions
        // global at the top level.  Instead we create a copy of those
        // groups here and deglobalize the arguments before adding
        // subcommands.  Various subcommand trees should then add
        //
        //    #[command(flatten)]
        //    pub conn: ConnectionOptions,
        //
        // to enable connection and/or cloud options for themselves
        // and their subcommands.
        let tmp = clap::Command::new("edgedb");
        let tmp = <RawOptions as clap::Args>::augment_args(tmp);
        let mut global_args: Vec<_> = tmp.get_groups()
            .filter(|g| g.get_id() == "connopts" || g.get_id() == "cloudopts")
            .flat_map(|g| g.get_args())
            .collect();
        global_args.sort_unstable();

        let deglobalized = tmp.get_arguments()
            .map(
                |arg|
                if global_args.binary_search(&arg.get_id()).is_ok() {
                    arg.clone().global(false)
                } else {
                    arg.clone()
                }
            );

        let app = clap::Command::new("edgedb")
                    .term_width(term_width())
                    .args(deglobalized);

        let app = <SubcommandOption as clap::Args>::augment_args(app);
        update_main_help(app)
    }

    pub fn from_args_and_env() -> anyhow::Result<Options> {
        let app = Options::command();
        let matches = app.clone().get_matches();
        let args = <RawOptions as clap::FromArgMatches>
            ::from_arg_matches(&matches)?;
        let cmd = <SubcommandOption as clap::FromArgMatches>
            ::from_arg_matches(&matches)?;

        let subcommand = cmd.subcommand;

        if args.help_connect {
            print_full_connection_options();
            return Err(ExitCode::new(0).into());
        }

        if args.print_version {
            println!("EdgeDB CLI {}", clap::crate_version!());
            return Err(ExitCode::new(0).into());
        }

        if subcommand.is_some() && args.query.is_some() {
            anyhow::bail!(
                "Option `-c` conflicts with specifying a subcommand"
            );
        }

        // TODO(pc) add option to force interactive mode not on a tty (tests)
        let interactive = args.query.is_none()
            && subcommand.is_none()
            && stdin().is_terminal();

        if args.json {
            say_option_is_deprecated(
                "--json",
                "edgedb query --output-format=json");
        }
        if args.tab_separated {
            say_option_is_deprecated(
                "--tab-separated",
                "edgedb query --output-format=tab-separated");
        }
        let subcommand = if let Some(query) = args.query {
            say_option_is_deprecated("-c", "edgedb query");
            let output_format = if args.json {
                Some(OutputFormat::Json)
            } else if args.tab_separated {
                Some(OutputFormat::TabSeparated)
            } else {
                Some(OutputFormat::JsonPretty)
            };
            Some(Command::Query(Query {
                queries: Some(vec![query]),
                output_format,
                file: None,
                conn: args.conn.clone(),
            }))
        } else {
            subcommand
        };

        let mut no_cli_update_check = args.no_cli_update_check;
        if args.no_version_check {
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
            app: app,
            conn_options: args.conn,
            cloud_options: args.cloud,
            interactive,
            subcommand,
            debug_print_frames: args.debug_print_frames,
            debug_print_descriptors: args.debug_print_descriptors,
            debug_print_codecs: args.debug_print_codecs,
            output_format: if args.tab_separated {
                Some(OutputFormat::TabSeparated)
            } else if args.json {
                Some(OutputFormat::Json)
            } else {
                None
            },
            no_cli_update_check,
            test_output_conn_params: args.test_output_conn_params,
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
    if let Some(val) = tmp.wait_until_available {
        bld.wait_until_available(val);
    }
    if let Some(val) = tmp.connect_timeout {
        bld.connect_timeout(val);
    }
    if let Some(val) = &tmp.secret_key {
        bld.secret_key(val);
    }

    if let Some(branch) = tmp.get_branch() {
        bld.database(branch)?;
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
    if let Some(tls_server_name) = &options.tls_server_name {
        builder.tls_server_name(tls_server_name)?;
    }
    Ok(())
}
