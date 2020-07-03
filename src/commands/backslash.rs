use std::borrow::Cow;
use std::collections::{BTreeSet, BTreeMap};

use anyhow;
use clap::{self, Clap, IntoApp};
use edgedb_protocol::server_message::ErrorResponse;
use once_cell::sync::Lazy;
use prettytable::{Table, Row, Cell};

use crate::commands::Options;
use crate::repl;
use crate::print::style::Styler;
use crate::prompt;
use crate::commands::execute;
use crate::commands::parser::{Backslash, BackslashCmd, Setting};
use crate::commands::type_names::get_type_names;
use crate::table;


pub static CMD_CACHE: Lazy<CommandCache> = Lazy::new(|| CommandCache::new());


pub enum ExecuteResult {
    Skip,
    Quit,
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

Editing
  \s, \history             show history
  \e, \edit [N]            spawn $EDITOR to edit history entry N then use the
                           output as the input

Settings
  \set [OPTION [VALUE]]    how/change setting, type \set for listing
                           all available options

Connection
  \c, \connect [DBNAME]    Connect to database DBNAME

Help
  \?, \h, \help            Show help on backslash commands
  \set                     Show setting descriptions (without arguments)
  \q, \quit, \exit, Ctrl+D Quit REPL
"###;

#[derive(Debug)]
pub struct ParseError {
    pub span: Option<(usize, usize)>,
    pub message: String,
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
pub struct Argument {
    pub required: bool,
    pub name: String,
}

#[derive(Debug)]
pub struct CommandInfo {
    pub options: String,
    pub arguments: Vec<Argument>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct SettingInfo {
    pub name: &'static str,
    pub description: String,
    pub name_description: String,
    pub setting: Setting,
    pub value_name: String,
    pub values: Option<Vec<String>>,
}

pub struct CommandCache {
    pub settings: BTreeMap<&'static str, SettingInfo>,
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
        aliases.insert("q", "exit");
        aliases.insert("quit", "exit");
        aliases.insert("?", "help");
        aliases.insert("h", "help");
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
                        .map(|a| Argument {
                            required: false,
                            name: a.get_name().to_owned(),
                        })
                        .collect(),
                    description: cmd.get_about().map(|x| x.to_owned()),
                })
            })
            .collect();
        let setting_cmd = setting_cmd.expect("set command exists");
        let mut setting_cmd: BTreeMap<_, _> = setting_cmd.get_subcommands()
            .iter()
            .map(|cmd| (cmd.get_name(), cmd))
            .collect();
        let settings = vec![
                InputMode(Default::default()),
                ImplicitProperties(Default::default()),
                IntrospectTypes(Default::default()),
                VerboseErrors(Default::default()),
                Limit(Default::default()),
                OutputMode(Default::default()),
                ExpandStrings(Default::default()),
                HistorySize(Default::default()),
            ].into_iter().map(|setting| {
                let cmd = setting_cmd.remove(&setting.name())
                    .expect("all settings have cmd");
                let arg = cmd.get_arguments().get(0)
                    .expect("setting has argument");
                let values = arg.get_possible_values()
                    .map(|v| v.iter().map(|x| (*x).to_owned()).collect());
                let description = cmd.get_about().unwrap_or("").to_owned();
                let info = SettingInfo {
                    name: setting.name(),
                    name_description: format!("{} -- {}",
                        setting.name(), description),
                    description,
                    setting,
                    value_name: arg.get_name().to_owned(),
                    values,
                 };
                (info.name, info)
            }).collect();
        CommandCache {
            settings,
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
                if x == "\\?" || x == "\\h" || x == "\\help" {
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
        HistorySize(_) => {
            prompt.history_limit.to_string().into()
        }
        OutputMode(_) => {
            prompt.output_mode.as_str().into()
        }
        ExpandStrings(_) => {
            bool_str(prompt.print.expand_strings).into()
        }
     }
}

fn list_settings(prompt: &mut repl::State) {
    let mut table = Table::new();
    table.set_format(*table::FORMAT);
    table.set_titles(Row::new(
        ["Setting", "Current", "Description"]
        .iter().map(|x| table::header_cell(x)).collect()));
    for setting in CMD_CACHE.settings.values() {
        table.add_row(Row::new(vec![
            Cell::new(&setting.name),
            Cell::new(&get_setting(&setting.setting, prompt)),
            Cell::new(&textwrap::fill(&setting.description, 40)),
        ]));
    }
    table.printstd();
}

pub async fn execute(cmd: &BackslashCmd, prompt: &mut repl::State)
    -> Result<ExecuteResult, anyhow::Error>
{
    use crate::commands::parser::BackslashCmd::*;
    use crate::commands::parser::SetCommand;
    use Setting::*;
    use ExecuteResult::*;

    let options = Options {
        command_line: false,
        styler: Some(Styler::dark_256()),
        conn_params: prompt.conn_params.clone(),
    };
    let cli = prompt.connection.as_mut().expect("connection established");
    match cmd {
        Help => {
            print!("{}", HELP);
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
                    let limit = c.limit.expect("only set here");
                    if limit == 0 {
                        prompt.implicit_limit = None;
                        prompt.print.max_items = None;
                    } else {
                        prompt.implicit_limit = Some(limit);
                        prompt.print.max_items = Some(limit);
                    }
                }
                HistorySize(c) => {
                    let limit = c.value.expect("only set here");
                    prompt.set_history_limit(limit).await;
                }
                OutputMode(c) => {
                    prompt.output_mode = c.mode.expect("only writes here");
                }
                ExpandStrings(b) => {
                    prompt.print.expand_strings = b.unwrap_value();
                }
            }
            Ok(Skip)
        }
        Connect(c) => {
            if prompt.in_transaction() {
                eprintln!("WARNING: Transaction cancelled")
            }
            prompt.try_connect(&c.database_name).await
                .map_err(|e| {
                    eprintln!("Error: Cannot connect: {:#}", e)
                })
                .ok();
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
        Edit(c) => {
            match prompt.spawn_editor(c.entry).await {
                | prompt::Input::Text(text) => Ok(Input(text)),
                | prompt::Input::Interrupt
                | prompt::Input::Eof => Ok(Skip),
            }
        }
        Exit => Ok(Quit),
    }
}

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
