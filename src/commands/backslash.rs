use std::fmt;
use std::error::Error;

use anyhow;

use edgedb_protocol::server_message::ErrorResponse;

use crate::client::Client;
use crate::commands::{self, Options};
use crate::repl::{self, OutputMode};
use crate::print;
use crate::print::style::Styler;
use crate::prompt;
use crate::server_params::PostgresAddress;
use crate::commands::helpers::quote_name;
use crate::commands::type_names::get_type_names;

pub enum ExecuteResult {
    Skip,
    Input(String),
}

const HELP: &str = r###"
Introspection
  (options: + = verbose, S = show system objects, I = case-sensitive match)
  \d[+] NAME               describe schema object
  \l, \list-databases      list databases
  \lT[IS] [PATTERN]        list scalar types
                           (alias: \list-scalar-types)
  \lt[IS] [PATTERN]        list object types
                           (alias: \list-object-types)
  \lr[I]                   list roles
                           (alias: \list-roles)
  \lm[I]                   list modules
                           (alias: \list-modules)
  \la[IS+] [PATTERN]       list expression aliases
                           (alias: \list-aliases)
  \lc[I] [PATTERN]         list casts
                           (alias: \list-casts)
  \li[IS+] [PATTERN]       list indexes
                           (alias: \list-indexes)
  \list-ports              list ports

Operations
  \dump FILENAME           dump current database into a file
  \restore FILENAME        restore the database from file into the current one
  \create-database DBNAME  create a new database

Editing
  \s, \history             show history
  \e, \edit [N]            spawn $EDITOR to edit history entry N then use the
                           output as the input

Settings
  \limit [LIMIT]           Set implicit LIMIT. Defaults to 100, specify 0 to disable.
  \output [MODE]           Set output mode. One of:
                             json, json-elements, default, tab-separated
  \vi                      switch to vi-mode editing
  \emacs                   switch to emacs (normal) mode editing, disables vi-mode
  \implicit-properties     print implicit properties of objects (id, type id)
  \no-implicit-properties  disable printing implicit properties
  \introspect-types        print typenames instead of `Object` (may fail if
                           schema is updated after enabling option)
  \no-introspect-types     disable type introspection
  \verbose-errors          print all errors with maximum verbosity
  \no-verbose-errors       only print InternalServerError with maximum verbosity

Connection
  \c, \connect [DBNAME]    Connect to database DBNAME

Development
  \E                       show most recent error message at maximum verbosity
                           (alias: \last-error)
  \pgaddr                  show the network addr of the postgres server
  \psql                    open psql to the current postgres process

Help
  \?                       Show help on backslash commands
"###;

pub const HINTS: &'static [&'static str] = &[
    r"\?",
    r"\c DBNAME",
    r"\connect DBNAME",
    r"\create-database DBNAME",
    r"\d NAME",
    r"\d NAME",
    r"\d+ NAME",
    r"\describe NAME",
    r"\describe+ NAME",
    r"\dump FILENAME",
    r"\e [N]",
    r"\edit [N]",
    r"\emacs",
    r"\history",
    r"\implicit-properties",
    r"\introspect-types",
    r"\l",
    r"\la [PATTERN]",
    r"\laI [PATTERN]",
    r"\laIS [PATTERN]",
    r"\laS [PATTERN]",
    r"\laSI [PATTERN]",
    r"\la+ [PATTERN]",
    r"\laI+ [PATTERN]",
    r"\laIS+ [PATTERN]",
    r"\laS+ [PATTERN]",
    r"\laSI+ [PATTERN]",
    r"\last-error",
    r"\lT [PATTERN]",
    r"\lc [PATTERN]",
    r"\lcI [PATTERN]",
    r"\li [PATTERN]",
    r"\li+ [PATTERN]",
    r"\liI [PATTERN]",
    r"\liI+ [PATTERN]",
    r"\liIS [PATTERN]",
    r"\liIS+ [PATTERN]",
    r"\liS [PATTERN]",
    r"\liS+ [PATTERN]",
    r"\liSI [PATTERN]",
    r"\liSI+ [PATTERN]",
    r"\limit [LIMIT]",
    r"\lTI [PATTERN]",
    r"\lTIS [PATTERN]",
    r"\lTS [PATTERN]",
    r"\lTSI [PATTERN]",
    r"\lt [PATTERN]",
    r"\ltI [PATTERN]",
    r"\ltIS [PATTERN]",
    r"\ltS [PATTERN]",
    r"\ltSI [PATTERN]",
    r"\list-aliases [PATTERN]",
    r"\list-casts [PATTERN]",
    r"\list-databases",
    r"\list-indexes [PATTERN]",
    r"\list-modules [PATTERN]",
    r"\list-object-types [PATTERN]",
    r"\list-ports",
    r"\list-roles [PATTERN]",
    r"\list-scalar-types [PATTERN]",
    r"\lr",
    r"\lrI",
    r"\no-implicit-properties",
    r"\no-introspect-types",
    r"\no-verbose-errors",
    r"\output [json|json-elements|default|tab-separated]",
    r"\pgaddr",
    r"\psql",
    r"\restore FILENAME",
    r"\s",
    r"\verbose-errors",
    r"\vi",
];

pub const COMMAND_NAMES: &'static [&'static str] = &[
    r"\?",
    r"\c",
    r"\connect",
    r"\create-database",
    r"\d",
    r"\d+",
    r"\describe",
    r"\describe+",
    r"\dump",
    r"\e",
    r"\edit",
    r"\emacs",
    r"\implicit-properties",
    r"\introspect-types",
    r"\history",
    r"\l",
    r"\la",
    r"\la+",
    r"\laI",
    r"\laI+",
    r"\laIS",
    r"\laIS+",
    r"\laS",
    r"\laS+",
    r"\laSI",
    r"\laSI+",
    r"\last-error",
    r"\li",
    r"\liI",
    r"\liIS",
    r"\liS",
    r"\liSI",
    r"\li+",
    r"\liI+",
    r"\liIS+",
    r"\liS+",
    r"\liSI+",
    r"\limit",
    r"\lc",
    r"\lcI",
    r"\lT",
    r"\lTI",
    r"\lTIS",
    r"\lTS",
    r"\lTSI",
    r"\lt",
    r"\ltI",
    r"\ltIS",
    r"\ltS",
    r"\ltSI",
    r"\list-aliases",
    r"\list-casts",
    r"\list-databases",
    r"\list-indexes",
    r"\list-modules",
    r"\list-ports",
    r"\list-roles",
    r"\list-object-types",
    r"\list-scalar-types",
    r"\lr",
    r"\lrI",
    r"\no-implicit-properties",
    r"\no-introspect-types",
    r"\no-verbose-errors",
    r"\output default",
    r"\output json",
    r"\output json-elements",
    r"\output tab-separated",
    r"\output",
    r"\pgaddr",
    r"\psql",
    r"\restore",
    r"\s",
    r"\verbose-errors",
    r"\vi",
];

pub enum Command {
    Help,
    ListAliases {
        pattern: Option<String>,
        system: bool,
        case_sensitive: bool,
        verbose: bool,
    },
    ListCasts {
        pattern: Option<String>,
        case_sensitive: bool,
    },
    ListIndexes {
        pattern: Option<String>,
        system: bool,
        case_sensitive: bool,
        verbose: bool,
    },
    ListDatabases,
    ListPorts,
    ListModules {
        pattern: Option<String>,
        case_sensitive: bool,
    },
    ListRoles {
        pattern: Option<String>,
        case_sensitive: bool,
    },
    ListScalarTypes {
        pattern: Option<String>,
        system: bool,
        case_sensitive: bool,
    },
    ListObjectTypes {
        pattern: Option<String>,
        system: bool,
        case_sensitive: bool,
    },
    Describe {
        name: String,
        verbose: bool,
    },
    PostgresAddr,
    Psql,
    ViMode,
    EmacsMode,
    ImplicitProperties,
    NoImplicitProperties,
    IntrospectTypes,
    NoIntrospectTypes,
    Connect { database: String },
    LastError,
    VerboseErrors,
    NoVerboseErrors,
    History,
    Edit { entry: Option<isize> },
    SetLimit { value: usize },
    ShowLimit,
    SetOutput { mode: OutputMode },
    ShowOutput,
    Dump { filename: String },
    Restore { filename: String },
    CreateDatabase { name: String },
}

pub struct ParseError {
    pub message: String,
    pub hint: String,
}

#[derive(Debug)]
pub struct ChangeDb {
    pub target: String,
}

pub fn error<T, S: ToString>(message: S, hint: &str) -> Result<T, ParseError> {
    Err(ParseError {
        message: message.to_string(),
        hint: hint.into(),
    })
}

pub fn parse(s: &str) -> Result<Command, ParseError> {
    let s = s.trim_start();
    if !s.starts_with("\\") {
        return error("Backslash command must start with a backslash", "");
    }
    let cmd = match s[1..].split_whitespace().next() {
        Some(cmd) => cmd,
        None => return error("Empty command", ""),
    };
    let arg = s[1+cmd.len()..].trim_start();
    let arg = if arg.len() > 0 { Some(arg) } else { None };
    match (cmd, arg) {
        ("?", None) => Ok(Command::Help),
        | ("list-databases", None)
        | ("l", None)
        => Ok(Command::ListDatabases),
        | ("list-ports", None)
        => Ok(Command::ListPorts),
        | ("list-casts", pattern)
        | ("lc", pattern)
        | ("lcI", pattern)
        => Ok(Command::ListCasts {
            pattern: pattern.map(|x| x.to_owned()),
            case_sensitive: cmd.contains('I'),
        }),
        | ("list-aliases", pattern)
        | ("la", pattern)
        | ("laI", pattern)
        | ("laS", pattern)
        | ("laIS", pattern)
        | ("laSI", pattern)
        | ("la+", pattern)
        | ("laI+", pattern)
        | ("laS+", pattern)
        | ("laIS+", pattern)
        | ("laSI+", pattern)
        => Ok(Command::ListAliases {
            pattern: pattern.map(|x| x.to_owned()),
            system: cmd.contains('S'),
            case_sensitive: cmd.contains('I'),
            verbose: cmd.contains('+'),
        }),
        | ("list-indexes", pattern)
        | ("li", pattern)
        | ("liI", pattern)
        | ("liS", pattern)
        | ("liIS", pattern)
        | ("liSI", pattern)
        | ("li+", pattern)
        | ("liI+", pattern)
        | ("liS+", pattern)
        | ("liIS+", pattern)
        | ("liSI+", pattern)
        => Ok(Command::ListIndexes {
            pattern: pattern.map(|x| x.to_owned()),
            system: cmd.contains('S'),
            case_sensitive: cmd.contains('I'),
            verbose: cmd.contains('+'),
        }),
        | ("list-scalar-types", pattern)
        | ("lT", pattern)
        | ("lTI", pattern)
        | ("lTS", pattern)
        | ("lTIS", pattern)
        | ("lTSI", pattern)
        => Ok(Command::ListScalarTypes {
            pattern: pattern.map(|x| x.to_owned()),
            system: cmd.contains('S'),
            case_sensitive: cmd.contains('I'),
        }),
        | ("lt", pattern)
        | ("ltI", pattern)
        | ("ltS", pattern)
        | ("ltIS", pattern)
        | ("ltSI", pattern)
        => Ok(Command::ListObjectTypes {
            pattern: pattern.map(|x| x.to_owned()),
            system: cmd.contains('S'),
            case_sensitive: cmd.contains('I'),
        }),
        | ("list-roles", pattern)
        | ("lr", pattern)
        | ("lrI", pattern)
        => Ok(Command::ListRoles {
            pattern: pattern.map(|x| x.to_owned()),
            case_sensitive: cmd.contains('I'),
        }),
        | ("list-modules", pattern)
        | ("lm", pattern)
        | ("lmI", pattern)
        => Ok(Command::ListModules {
            pattern: pattern.map(|x| x.to_owned()),
            case_sensitive: cmd.contains('I'),
        }),
        | ("c", Some(database))
        | ("connect", Some(database))
        => Ok(Command::Connect { database: database.to_owned() }),
        | ("describe", Some(name))
        | ("d", Some(name))
        => Ok(Command::Describe { name: name.to_owned(), verbose: false}),
        | ("describe+", Some(name))
        | ("d+", Some(name))
        => Ok(Command::Describe { name: name.to_owned(), verbose: true}),
        | ("last-error", None)
        | ("E", None)
        => Ok(Command::LastError),
        | ("history", None)
        | ("s", None)
        => Ok(Command::History),
        | ("edit", param)
        | ("e", param)
        => Ok(Command::Edit {
            entry: param.map(|x| x.parse()).transpose()
                .map_err(|e| ParseError {
                    message: format!("bad number: {}", e),
                    hint: "integer expected".into(),
                })?,
        }),
        ("pgaddr", None) => Ok(Command::PostgresAddr),
        ("psql", None) => Ok(Command::Psql),
        ("vi", None) => Ok(Command::ViMode),
        ("emacs", None) => Ok(Command::EmacsMode),
        ("limit", Some(value)) => Ok(Command::SetLimit {
            value: value.parse().map_err(|e| ParseError {
                message: format!("bad number: {}", e),
                hint: "integer expected".into(),
            })?,
        }),
        ("limit", None) => Ok(Command::ShowLimit),
        ("implicit-properties", None) => Ok(Command::ImplicitProperties),
        ("no-implicit-properties", None) => Ok(Command::NoImplicitProperties),
        ("introspect-types", None) => Ok(Command::IntrospectTypes),
        ("no-introspect-types", None) => Ok(Command::NoIntrospectTypes),
        ("verbose-errors", None) => Ok(Command::VerboseErrors),
        ("no-verbose-errors", None) => Ok(Command::NoVerboseErrors),
        ("dump", Some(param)) => Ok(Command::Dump { filename: param.into() }),
        ("restore", Some(param)) => {
            Ok(Command::Restore { filename: param.into() })
        }
        ("create-database", Some(name)) => {
            Ok(Command::CreateDatabase { name: name.into() })
        }
        ("output", None) => Ok(Command::ShowOutput),
        ("output", Some(param)) => {
            Ok(Command::SetOutput {
                mode: match param {
                    "json" => OutputMode::Json,
                    "json-elements" => OutputMode::JsonElements,
                    "default" => OutputMode::Default,
                    "tab-separated" => OutputMode::TabSeparated,
                    _ => return Err(ParseError {
                        message: format!("invalid output format: {}", param),
                        hint: "expected one of: json, json-elements, \
                            default, tab-separated".into(),
                    }),
                },
            })
        }
        (_, Some(_)) if COMMAND_NAMES.contains(&&s[..cmd.len()+1]) => {
            error(format_args!("Command `\\{}` doesn't support arguments",
                               cmd.escape_default()),
                  "no argument expected")
        }
        (_, None) if COMMAND_NAMES.contains(&&s[..cmd.len()+1]) => {
            error(format_args!("Command `\\{}` requires an argument",
                               cmd.escape_default()),
                  "add an argument")
        }
        (_, _) => {
            error(format_args!("Unknown command `\\{}'", cmd.escape_default()),
                  "unknown command")
        }
    }
}

pub async fn execute<'x>(cli: &mut Client<'x>, cmd: Command,
    prompt: &mut repl::State)
    -> Result<ExecuteResult, anyhow::Error>
{
    use Command::*;
    use ExecuteResult::*;
    let options = Options {
        command_line: false,
        styler: Some(Styler::dark_256()),
    };
    match cmd {
        Help => {
            print!("{}", HELP);
            Ok(Skip)
        }
        ListAliases { pattern, case_sensitive, system, verbose } => {
            commands::list_aliases(cli, &options,
                &pattern, system, case_sensitive, verbose).await?;
            Ok(Skip)
        }
        ListCasts { pattern, case_sensitive } => {
            commands::list_casts(cli, &options,
                &pattern, case_sensitive).await?;
            Ok(Skip)
        }
        ListIndexes { pattern, case_sensitive, system, verbose } => {
            commands::list_indexes(cli, &options,
                &pattern, system, case_sensitive, verbose).await?;
            Ok(Skip)
        }
        ListDatabases => {
            commands::list_databases(cli, &options).await?;
            Ok(Skip)
        }
        ListPorts => {
            commands::list_ports(cli, &options).await?;
            Ok(Skip)
        }
        ListScalarTypes { pattern, case_sensitive, system } => {
            commands::list_scalar_types(cli, &options,
                &pattern, system, case_sensitive).await?;
            Ok(Skip)
        }
        ListObjectTypes { pattern, case_sensitive, system } => {
            commands::list_object_types(cli, &options,
                &pattern, system, case_sensitive).await?;
            Ok(Skip)
        }
        ListModules { pattern, case_sensitive } => {
            commands::list_modules(cli, &options,
                &pattern, case_sensitive).await?;
            Ok(Skip)
        }
        ListRoles { pattern, case_sensitive } => {
            commands::list_roles(cli, &options,
                &pattern, case_sensitive).await?;
            Ok(Skip)
        }
        PostgresAddr => {
            match cli.params.get::<PostgresAddress>() {
                Some(addr) => {
                    println!("{}", serde_json::to_string_pretty(addr)?);
                }
                None => {
                    eprintln!("\\pgaddr requires EdgeDB to run in DEV mode");
                }
            }
            Ok(Skip)
        }
        Psql => {
            commands::psql(cli, &options).await?;
            Ok(Skip)
        }
        ViMode => {
            prompt.vi_mode().await;
            Ok(Skip)
        }
        EmacsMode => {
            prompt.emacs_mode().await;
            Ok(Skip)
        }
        ImplicitProperties => {
            prompt.print.implicit_properties = true;
            Ok(Skip)
        }
        NoImplicitProperties => {
            prompt.print.implicit_properties = true;
            Ok(Skip)
        }
        IntrospectTypes => {
            prompt.print.type_names = Some(get_type_names(cli).await?);
            Ok(Skip)
        }
        NoIntrospectTypes => {
            prompt.print.type_names = None;
            Ok(Skip)
        }
        Describe { name, verbose } => {
            commands::describe(cli, &options, &name, verbose).await?;
            Ok(Skip)
        }
        Connect { database } => {
            Err(ChangeDb { target: database })?
        }
        VerboseErrors => {
            prompt.verbose_errors = true;
            Ok(Skip)
        }
        NoVerboseErrors => {
            prompt.verbose_errors = false;
            Ok(Skip)
        }
        LastError => {
            if let Some(ref err) = prompt.last_error {
                if let Some(ref err) = err.downcast_ref::<ErrorResponse>() {
                    println!("{}", err.display_verbose());
                } else {
                    println!("{:#?}", err);
                }
            } else {
                eprintln!("== there is no previous error ==");
            }
            Ok(Skip)
        }
        History => {
            prompt.show_history().await;
            Ok(Skip)
        }
        Edit { entry } => {
            match prompt.spawn_editor(entry).await {
                | prompt::Input::Text(text) => Ok(Input(text)),
                | prompt::Input::Interrupt
                | prompt::Input::Eof => Ok(Skip),
            }
        }
        SetLimit { value } => {
            if value == 0 {
                prompt.implicit_limit = None;
                prompt.print.max_items = None;
            } else {
                prompt.implicit_limit = Some(value);
                prompt.print.max_items = Some(value);
            }
            Ok(Skip)
        }
        ShowLimit => {
            if let Some(limit) = prompt.implicit_limit {
                println!("{}", limit);
            } else {
                eprintln!("No limit");
            }
            Ok(Skip)
        }
        SetOutput { mode } => {
            prompt.output_mode = mode;
            Ok(Skip)
        }
        ShowOutput => {
            println!("{}", match prompt.output_mode {
                OutputMode::Json => "json",
                OutputMode::JsonElements => "json-elements",
                OutputMode::Default => "default",
                OutputMode::TabSeparated => "tab-separated",
            });
            Ok(Skip)
        }
        Dump { filename } => {
            commands::dump(cli, &options, filename.as_ref()).await?;
            Ok(Skip)
        }
        Restore { filename } => {
            commands::restore(cli, &options, filename.as_ref(), false).await?;
            Ok(Skip)
        }
        CreateDatabase { name } => {
            print::completion(&cli.execute(
                &format!("CREATE DATABASE {}", quote_name(&name))
            ).await?);
            Ok(Skip)
        }
    }
}

impl fmt::Display for ChangeDb {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "switch database to {:?}", self.target)
    }
}
impl Error for ChangeDb {}
