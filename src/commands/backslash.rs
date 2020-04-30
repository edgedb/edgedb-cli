use std::fmt;
use std::borrow::Cow;
use std::error::Error;
use std::collections::{BTreeSet, BTreeMap};

use anyhow;
use clap::{self, Clap, IntoApp};
use edgedb_protocol::server_message::ErrorResponse;
use prettytable::{Table, Row, Cell};

use crate::client::Client;
use crate::commands::Options;
use crate::repl;
use crate::print::style::Styler;
use crate::prompt;
use crate::commands::execute;
use crate::commands::parser::{Backslash, BackslashCmd, Setting};
use crate::commands::table;
use crate::commands::type_names::get_type_names;


lazy_static::lazy_static! {
    pub static ref CMD_CACHE: CommandCache = CommandCache::new();
}


pub enum ExecuteResult {
    Skip,
    Input(String),
}

const HELP: &str = r###"
Introspection
  (options: -v = verbose, -s = show system objects, -I = case-sensitive match)
  \d [-v] NAME             describe schema object
  \l, \list-databases      list databases
  \lT [-sI] [PATTERN]      list scalar types
                           (alias: \list-scalar-types)
  \lt [-sI] [PATTERN]      list object types
                           (alias: \list-object-types)
  \lr [-I]                 list roles
                           (alias: \list-roles)
  \lm [-I]                 list modules
                           (alias: \list-modules)
  \la [-Isv] [PATTERN]     list expression aliases
                           (alias: \list-aliases)
  \lc [-I] [PATTERN]       list casts
                           (alias: \list-casts)
  \li [-Isv] [PATTERN]     list indexes
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
  \set [OPTION [VALUE]]      show/change setting, see \

Connection
  \c, \connect [DBNAME]    Connect to database DBNAME
"###;

#[cfg(feature="dev_mode")]
const HELP_DEV: &str = r###"
Development
  \E                       show most recent error message at maximum verbosity
                           (alias: \last-error)
  \pgaddr                  show the network addr of the postgres server
  \psql                    open psql to the current postgres process
"###;

const HELP_HELP: &str = r###"
Help
  \?                       Show help on backslash commands
  \set                     Show setting descriptions (without arguments)
"###;

#[derive(Debug)]
pub struct ParseError {
    pub span: Option<(usize, usize)>,
    pub message: String,
}

#[derive(Debug)]
pub struct ChangeDb {
    pub target: String,
}

#[derive(Debug, PartialEq)]
pub struct Token<'a> {
    pub item: Item<'a>,
    pub span: (usize, usize),
}

#[derive(Debug, PartialEq)]
pub enum Item<'a> {
    Command(&'a str),
    Argument(&'a str),
    Error { message: &'a str },
    Incomplete { message: &'a str },
    Semicolon,
    Newline,
}

pub struct Parser<'a> {
    data: &'a str,
    first_item: bool,
    offset: usize,
}

#[derive(Debug)]
pub struct CommandInfo {
    pub options: String,
    pub arguments: Vec<String>,
    pub description: Option<String>,
}

pub struct CommandCache {
    pub settings: Vec<(Setting, String)>,
    pub commands: BTreeMap<String, CommandInfo>,
    pub aliases: BTreeMap<&'static str, &'static str>,
    pub all_commands: BTreeSet<String>,
}

impl<'a> Parser<'a> {
    pub fn new(s: &'a str) -> Parser<'a> {
        Parser {
            data: s,
            first_item: true,
            offset: 0,
        }
    }
    fn token(&self) -> Option<Token<'a>> {
        let whitespace: &[_] = &[' ', '\t'];
        let tail = self.data[self.offset..].trim_start_matches(whitespace);
        if tail.is_empty() {
            return None;
        }
        let offset = self.data.len() - tail.len();
        let mut iter = tail.char_indices();
        let end = loop {
            let (idx, c) = match iter.next() {
                Some(pair) => pair,
                None => break tail.len(),
            };
            match c {
                '\'' | '"' | '`' => {
                    let quote = c;
                    loop {
                        match iter.next() {
                            Some((_, c)) if c == quote => break,
                            Some((end, '\n')) | Some((end, '\r')) => {
                                return Some(Token {
                                    item: Item::Error {
                                        message: match quote {
                                            '\'' =>
                                                "expected end of single \
                                                quote `'` , got end of line",
                                            '"' =>
                                                "expected end of double \
                                                quote `\"` , got end of line",
                                            '`' =>
                                                "expected end of backtick \
                                                quote '`' , got end of line",
                                            _ => unreachable!(),
                                        },
                                    },
                                    span: (offset+idx, offset+end),
                                })
                            }
                            Some((_, _)) => {}
                            None => return Some(Token {
                                item: Item::Incomplete {
                                    message: match quote {
                                        '\'' => "incomplete 'single-quoted' \
                                                argument",
                                        '"' => "incomplete \"double-quoted\" \
                                                argument",
                                        '`' => "incomplete `backtick-quoted` \
                                                argument",
                                        _ => unreachable!(),
                                    },
                                },
                                span: (offset, self.data.len()),
                            }),
                        }
                    }
                }
                ';' if idx == 0 => {
                    return Some(Token {
                        item: Item::Semicolon,
                        span: (offset, offset+1),
                    });
                }
                '\n' if idx == 0 => {
                    return Some(Token {
                        item: Item::Newline,
                        span: (offset, offset+1),
                    });
                }
                '\r' if idx == 0 => {
                    let ln = if let Some((_, '\n')) = iter.next() {
                        2
                    } else {
                        1
                    };
                    return Some(Token {
                        item: Item::Newline,
                        span: (offset, offset+ln),
                    });
                }
                ' ' | '\t' | '\r' | '\n' | ';' => break idx,
                _ => {}
            }
        };
        let value = &tail[..end];
        let item = if self.first_item {
            if !value.starts_with('\\') {
                let char_len = value.chars().next().unwrap().len_utf8();
                return Some(Token {
                    item: Item::Error {
                        message: "command must start with backslash `\\`",
                    },
                    span: (offset, offset+char_len),
                })
            }
            Item::Command(value)
        } else {
            Item::Argument(value)
        };
        return Some(Token {
            item,
            span: (offset, offset+end),
        })
    }
}

impl<'a> Iterator for Parser<'a> {
    type Item = Token<'a>;
    fn next(&mut self) -> Option<Token<'a>> {
        let result = self.token();
        if let Some(ref tok) = result {
            if self.first_item {
                self.first_item = false;
            }
            self.offset = tok.span.1;
        }
        return result;
    }
}

impl CommandCache {
    fn new() -> CommandCache {
        use Setting::*;

        let clap = Backslash::into_app();
        let mut aliases = BTreeMap::new();
        aliases.insert("d", "describe");
        aliases.insert("l", "list-databases");
        aliases.insert("lT", "list-scalar-types");
        aliases.insert("lt", "list-object-types");
        aliases.insert("lr", "list-roles");
        aliases.insert("lm", "list-modules");
        aliases.insert("la", "list-aliases");
        aliases.insert("lc", "list-casts");
        aliases.insert("li", "list-indexes");
        aliases.insert("s", "history");
        aliases.insert("e", "edit");
        aliases.insert("c", "connect");
        aliases.insert("E", "last-error");
        aliases.insert("?", "help");
        let mut setting_cmd = None;
        let commands: BTreeMap<_,_> = clap.get_subcommands().iter()
            .map(|cmd| {
                let name = cmd.get_name().to_owned();
                if name == "set" {
                    setting_cmd = Some(cmd);
                }
                (name, CommandInfo {
                    options: cmd.get_arguments().iter()
                        .filter_map(|a| a.get_short())
                        .collect(),
                    arguments: cmd.get_arguments().iter()
                        .filter(|a| a.get_short().is_none())
                        .filter(|a| a.get_long().is_none())
                        .map(|a| a.get_name().to_owned())
                        .collect(),
                    description: cmd.get_about().map(|x| x.to_owned()),
                })
            })
            .collect();
        let setting_cmd = setting_cmd.expect("set command exists");
        let mut setting_descr: BTreeMap<_, _> = setting_cmd.get_subcommands()
            .iter()
            .map(|cmd| {
                (cmd.get_name(),
                 cmd.get_about().unwrap_or("").to_owned())
            })
            .collect();
        CommandCache {
            settings: vec![
                InputMode(Default::default()),
                ImplicitProperties(Default::default()),
                IntrospectTypes(Default::default()),
                VerboseErrors(Default::default()),
                Limit(Default::default()),
                OutputMode(Default::default()),
            ].into_iter().map(|s| {
                let descr = setting_descr.remove(&s.name())
                    .expect("all settings described");
                (s, descr)
            }).collect(),
            all_commands: commands.keys().map(|x| &x[..])
                .chain(aliases.keys().map(|x| *x))
                .map(|n| String::from("\\") + n)
                .collect(),
            commands,
            aliases,
        }
    }
}

pub fn full_statement(s: &str) -> usize {
    for token in Parser::new(s) {
        match token.item {
            Item::Semicolon | Item::Newline => return token.span.1,
            _ => {}
        }
    }
    return s.len();
}

pub fn parse(s: &str) -> Result<Backslash, ParseError> {
    use Item::*;

    let mut arguments = Vec::new();
    for token in Parser::new(s) {
        match token.item {
            Command(x) => {
                if x == "\\?" || x == "\\help" {
                    return Ok(Backslash {
                        command: BackslashCmd::Help,
                    })
                }
                if let Some(cmd) = CMD_CACHE.aliases.get(&x[1..]) {
                    arguments.push(cmd.to_string())
                } else {
                    arguments.push(x[1..].to_owned())
                }
            }
            Argument(x) => arguments.push(unquote_argument(x)),
            Newline | Semicolon => break,
            Incomplete { message } => {
                return Err(ParseError {
                    message: message.to_string(),
                    span: Some(token.span),
                })
            }
            Error { message } => {
                return Err(ParseError {
                    message: message.to_string(),
                    span: Some(token.span),
                })
            }
        }
    }
    Backslash::try_parse_from(arguments)
    .map_err(|e| ParseError {
        message: if e.cause.is_empty() {
            e.to_string()
        } else {
            e.cause
        },
        span: None,
    })
}

fn unquote_argument(s: &str) -> String {
    let mut buf = String::with_capacity(s.len());
    let mut iter = s.chars();
    while let Some(c) = iter.next() {
        match c {
            '\'' => {
                for c in &mut iter {
                    if c == '\'' { break; }
                    buf.push(c);
                }
            }
            '"' => {
                for c in &mut iter {
                    if c == '"' { break; }
                    buf.push(c);
                }
            }
            '`' => {
                for c in &mut iter {
                    if c == '`' { break; }
                    buf.push(c);
                }
            }
            _ => buf.push(c),
        }
    }
    return buf;
}

pub fn is_valid_command(s: &str) -> bool {
    CMD_CACHE.all_commands.get(s).is_some()
}

pub fn is_valid_prefix(s: &str) -> bool {
    let mut iter = CMD_CACHE.all_commands.range(s.to_string()..);
    match iter.next() {
        Some(cmd) => cmd.starts_with(s),
        None => false,
    }
}

pub fn bool_str(val: bool) -> &'static str {
    match val {
        true => "on",
        false => "off",
    }
}

pub fn get_setting(s: &Setting, prompt: &repl::State) -> Cow<'static, str> {
     use Setting::*;

     match s {
        InputMode(_) => {
            prompt.input_mode.as_str().into()
        }
        ImplicitProperties(_) => {
            bool_str(prompt.print.implicit_properties).into()
        }
        IntrospectTypes(_) => {
            bool_str(prompt.print.type_names.is_some()).into()
        }
        VerboseErrors(_) => {
            bool_str(prompt.verbose_errors).into()
        }
        Limit(_) => {
            if let Some(limit) = prompt.implicit_limit {
                limit.to_string().into()
            } else {
                "0  # no limit".into()
            }
        }
        OutputMode(_) => {
            prompt.output_mode.as_str().into()
        }
     }
}

fn list_settings(prompt: &mut repl::State) {
    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.set_titles(Row::new(
        ["Setting", "Current", "Description"]
        .iter().map(|x| table::header_cell(x)).collect()));
    for (ref cmd, ref description) in &CMD_CACHE.settings {
        table.add_row(Row::new(vec![
            Cell::new(cmd.name()),
            Cell::new(&get_setting(cmd, prompt)),
            Cell::new(&textwrap::fill(&description, 40)),
        ]));
    }
    table.printstd();
}

pub async fn execute<'x>(cli: &mut Client<'x>, cmd: &BackslashCmd,
    prompt: &mut repl::State)
    -> Result<ExecuteResult, anyhow::Error>
{
    use crate::commands::parser::BackslashCmd::*;
    use crate::commands::parser::SetCommand;
    use Setting::*;
    use ExecuteResult::*;

    let options = Options {
        command_line: false,
        styler: Some(Styler::dark_256()),
    };
    match cmd {
        Help => {
            print!("{}", HELP);
            #[cfg(feature="dev_mode")]
            print!("{}", HELP_DEV);
            print!("{}", HELP_HELP);
            Ok(Skip)
        }
        Common(ref cmd) => {
            execute::common(cli, cmd, &options).await?;
            Ok(Skip)
        }
        Set(SetCommand {setting: None}) => {
            list_settings(prompt);
            Ok(Skip)
        }
        Set(SetCommand {setting: Some(ref cmd)}) if cmd.is_show() => {
            println!("{}: {}", cmd.name(), get_setting(&cmd, prompt));
            Ok(Skip)
        }
        Set(SetCommand {setting: Some(ref cmd)}) => {
            match cmd {
                InputMode(m) => {
                    prompt.input_mode(m.mode.expect("only writes here")).await;
                }
                ImplicitProperties(b) => {
                    prompt.print.implicit_properties = b.unwrap_value();
                }
                IntrospectTypes(b) => {
                    if b.unwrap_value() {
                        prompt.print.type_names =
                            Some(get_type_names(cli).await?);
                    } else {
                        prompt.print.type_names = None;
                    }
                }
                VerboseErrors(b) => {
                    prompt.verbose_errors = b.unwrap_value();
                }
                Limit(c) => {
                    if let Some(limit) = c.limit {
                        if limit == 0 {
                            prompt.implicit_limit = None;
                            prompt.print.max_items = None;
                        } else {
                            prompt.implicit_limit = Some(limit);
                            prompt.print.max_items = Some(limit);
                        }
                    } else {
                        if let Some(limit) = prompt.implicit_limit {
                            println!("{}", limit);
                        } else {
                            eprintln!("No limit");
                        }
                    }
                }
                OutputMode(c) => {
                    prompt.output_mode = c.mode.expect("only writes here");
                }
            }
            Ok(Skip)
        }
        Connect(c) => {
            Err(ChangeDb { target: c.database_name.clone() })?
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
        Edit(c) => {
            match prompt.spawn_editor(c.entry).await {
                | prompt::Input::Text(text) => Ok(Input(text)),
                | prompt::Input::Interrupt
                | prompt::Input::Eof => Ok(Skip),
            }
        }
    }
}

impl fmt::Display for ChangeDb {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "switch database to {:?}", self.target)
    }
}
impl Error for ChangeDb {}

#[cfg(test)]
mod test {
    use super::Parser;
    use super::Item::{self, *};

    fn tok_values<'x>(s: &'x str) -> Vec<Item<'x>> {
        Parser::new(s).map(|tok| tok.item).collect::<Vec<_>>()
    }

    #[test]
    fn test_parser() {
        assert_eq!(tok_values("\\x"), [Command("\\x")]);
        assert_eq!(tok_values("\\x a b"),
            [Command("\\x"), Argument("a"), Argument("b")]);
        assert_eq!(tok_values("\\x 'a b'"),
            [Command("\\x"), Argument("'a b'")]);
        assert_eq!(tok_values("\\describe schema::`Object`"),
            [Command("\\describe"), Argument("schema::`Object`")]);
    }
}
