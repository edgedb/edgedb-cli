use std::borrow::Cow;
use std::fs;
use std::io::{ErrorKind, Write};
use std::env;
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::Context as _Context;
use dirs::data_local_dir;
use rustyline::completion::Completer;
use rustyline::config::{EditMode, CompletionType, Builder as ConfigBuilder};
use rustyline::highlight::{Highlighter, PromptInfo};
use rustyline::hint::Hinter;
use rustyline::history::History;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::{Editor, Config, Helper, Context};
use rustyline::{error::ReadlineError, KeyEvent, Modifiers, Cmd};
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot::Sender;

use edgeql_parser::preparser::full_statement;
use edgedb_protocol::value::Value;
use crate::commands::backslash;
use crate::completion;
use crate::print::Highlight;
use crate::print::style::Styler;
use crate::highlight;
use crate::prompt::variable::VariableInput;
use crate::repl::{TX_MARKER, FAILURE_MARKER};
use crate::platform::editor_path;

use colorful::Colorful;

pub mod variable;


pub enum Control {
    EdgeqlInput { prompt: String, initial: String, response: Sender<Input> },
    ParameterInput {
        name: String,
        var_type: Arc<dyn VariableInput>,
        optional: bool,
        initial: String,
        response: Sender<VarInput>,
    },
    ShowHistory { ack: Sender<()> },
    SpawnEditor { entry: Option<isize>, response: Sender<Input> },
    ViMode,
    EmacsMode,
    SetHistoryLimit(usize),
}

pub enum Input {
    Text(String),
    Eof,
    Interrupt,
}

pub enum VarInput {
    Value(Value),
    Eof,
    Interrupt,
}

pub struct EdgeqlHelper {
    styler: Styler,
}

impl Helper for EdgeqlHelper {}
impl Hinter for EdgeqlHelper {
    type Hint = completion::Hint;
    fn hint(&self, line: &str, pos: usize, _ctx: &Context)
        -> Option<Self::Hint>
    {
        completion::hint(line, pos)
    }
}

impl Highlighter for EdgeqlHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self,
        prompt: &'p str, info: PromptInfo<'_>,)
        -> Cow<'b, str>
    {
        if info.line_no() > 0 {
            format!("{0:.>1$}", " ", prompt.len()).into()
        } else if prompt.ends_with("> ") {
            let content = &prompt[..prompt.len()-2];
            if content.ends_with(TX_MARKER) {
                return format!("{}{}> ",
                    &content[..content.len()-TX_MARKER.len()],
                    TX_MARKER.green()).into();
            } else if content.ends_with(FAILURE_MARKER) {
                return format!("{}{}> ",
                    &content[..content.len()-FAILURE_MARKER.len()],
                    FAILURE_MARKER.red()).into();
            } else {
                return prompt.into();
            }
        } else {
            return prompt.into();
        }
    }
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let mut buf = String::with_capacity(line.len() + 8);
        let mut data = line;
        loop {
            if data.trim().is_empty() {
                buf.push_str(data);
                return buf.into();
            }
            if data.trim_start().starts_with('\\') {
                let bytes = backslash::full_statement(data);
                highlight::backslash(&mut buf, &data[..bytes], &self.styler);
                data = &data[bytes..];
            } else {
                match full_statement(data.as_bytes(), None) {
                    Ok(bytes) => {
                        highlight::edgeql(&mut buf,
                            &data[..bytes], &self.styler);
                        data = &data[bytes..];
                    }
                    Err(_cont) => {
                        highlight::edgeql(&mut buf,
                            data, &self.styler);
                        data = "";
                    }
                }
            }
        }
    }
    fn highlight_char(&self, _line: &str, _pos: usize) -> bool {
        // TODO(tailhook) optimize: only need to return true on insert
        true
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        hint.rgb(0x56, 0x56, 0x56).to_string().into()
    }
    fn highlight_candidate<'h>(&self, item: &'h str, _typ: CompletionType)
        -> std::borrow::Cow<'h, str>
    {
        use std::fmt::Write;

        if let Some(pos) = item.find(" -- ") {
            let mut buf = String::with_capacity(item.len() + 8);
            let (value, descr) = item.split_at(pos);
            buf.push_str(value);
            write!(buf, "{}", descr.light_gray()).unwrap();
            buf.into()
        } else {
            item.into()
        }
    }
    fn has_continuation_prompt(&self) -> bool {
        true
    }
}
impl Validator for EdgeqlHelper {
    fn validate(&self, ctx: &mut ValidationContext)
        -> Result<ValidationResult, ReadlineError>
    {
        let input = ctx.input();
        let complete = match completion::current(input, input.len()).1 {
            completion::Current::Edgeql(_, complete) => complete,
            completion::Current::Empty => true,
            completion::Current::Backslash(_) => true,
        };
        if complete {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete)
        }
    }
}
impl Completer for EdgeqlHelper {
    type Candidate = completion::Pair;
    fn complete(&self, line: &str, pos: usize, _ctx: &Context)
        -> Result<(usize, Vec<Self::Candidate>), ReadlineError>
    {
        let comp = completion::complete(line, pos);
        if let Some((offset, options)) = comp {
            Ok((offset, options))
        } else {
            Ok((pos, Vec::new()))
        }
    }
}

pub fn load_history<H: rustyline::Helper>(ed: &mut Editor<H>, name: &str)
    -> Result<(), anyhow::Error>
{
    let dir = data_local_dir().context("cannot find local data dir")?;
    let app_dir = dir.join("edgedb");
    match ed.load_history(&app_dir.join(format!("{}.history", name))) {
        Err(ReadlineError::Io(e)) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("error loading history")?,
        Ok(()) => {}
    }
    Ok(())
}

fn _save_history<H: Helper>(ed: &mut Editor<H>, name: &str)
    -> Result<(), anyhow::Error>
{
    let dir = data_local_dir().context("cannot find local data dir")?;
    let app_dir = dir.join("edgedb");
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir).context("cannot create application dir")?;
    }
    ed.save_history(&app_dir.join(format!("{}.history", name)))
        .context("error writing history file")?;
    Ok(())
}

pub fn save_history<H: Helper>(ed: &mut Editor<H>, name: &str) {
    _save_history(ed, name).map_err(|e| {
        log::warn!("Cannot save history: {:#}", e);
    }).ok();
}

pub fn create_editor(config: &ConfigBuilder) -> Editor<EdgeqlHelper> {
    let mut editor = Editor::<EdgeqlHelper>::with_config(
        config.clone().build());
    editor.bind_sequence(KeyEvent::new('\r', Modifiers::NONE),
        Cmd::AcceptOrInsertLine { accept_in_the_middle: false });
    editor.bind_sequence(KeyEvent::new('\r', Modifiers::ALT), Cmd::AcceptLine);
    load_history(&mut editor, "edgeql").map_err(|e| {
        log::warn!("Cannot load history: {:#}", e);
    }).ok();
    editor.set_helper(Some(EdgeqlHelper {
        styler: Styler::dark_256(),
    }));
    editor
}

pub fn var_editor(config: &ConfigBuilder, var_type: &Arc<dyn VariableInput>)
    -> Editor<variable::VarHelper>
{
    let mut editor = Editor::<variable::VarHelper>::with_config(
        config.clone().build());
    editor.set_helper(Some(variable::VarHelper::new(var_type.clone())));
    let history_name = format!("var_{}", var_type.type_name());
    load_history(&mut editor, &history_name).map_err(|e| {
        log::warn!("Cannot load history: {:#}", e);
    }).ok();
    editor
}

pub fn edgeql_input(prompt: &str, editor: &mut Editor<EdgeqlHelper>,
    response: Sender<Input>, initial: &str)
    -> anyhow::Result<()>
{
    let text = match
        editor.readline_with_initial(prompt, (initial, ""))
    {
        Ok(text) => text,
        Err(ReadlineError::Eof) => {
            response.send(Input::Eof).ok();
            return Ok(());
        }
        Err(ReadlineError::Interrupted) => {
            response.send(Input::Interrupt).ok();
            return Ok(());
        }
        Err(e) => {
            eprintln!("Readline error: {}", e);
            return Ok(());
        }
    };
    editor.add_history_entry(&text);
    response.send(Input::Text(text)).ok();
    save_history(editor, "edgeql");
    Ok(())
}

pub fn main(mut control: Receiver<Control>)
    -> Result<(), anyhow::Error>
{
    let config = Config::builder();
    let config = config.edit_mode(EditMode::Emacs);
    let mut config = config.completion_type(CompletionType::List);
    let mut editor = create_editor(&config);
    'outer: loop {
        match control.blocking_recv() {
            None => break 'outer,
            Some(Control::ViMode) => {
                config = config.edit_mode(EditMode::Vi);
                editor = create_editor(&config);
            }
            Some(Control::EmacsMode) => {
                config = config.edit_mode(EditMode::Emacs);
                editor = create_editor(&config);
            }
            Some(Control::SetHistoryLimit(h)) => {
                config = config.max_history_size(h);
                editor = create_editor(&config);
            }
            Some(Control::EdgeqlInput { prompt, initial, response }) => {
                edgeql_input(&prompt, &mut editor, response, &initial)?;
            }
            Some(Control::ParameterInput {
                name, var_type, optional, initial, response,
            }) => {
                let mut initial = initial;
                let prompt = format!(
                    "Parameter <{}>${}{}: ",
                    &var_type.type_name(), &name,
                    if optional {
                        " (Ctrl+D for empty set `{}`)".fade().to_string()
                    } else { String::new() },
                );
                let mut editor = var_editor(&config, &var_type);
                let (text, value) = loop {
                    let text = match
                        editor.readline_with_initial(&prompt, (&initial, ""))
                    {
                        Ok(text) => text,
                        Err(ReadlineError::Eof) => {
                            if optional {
                                response.send(VarInput::Eof).ok();
                                continue 'outer;
                            } else {
                                println!("Optional values are not supported \
                                    for this parameter. Use Ctrl+C to quit.");
                                continue;
                            }
                        }
                        Err(ReadlineError::Interrupted) => {
                            response.send(VarInput::Interrupt).ok();
                            continue 'outer;
                        }
                        Err(e) => Err(e)?,
                    };
                    match var_type.parse(&text) {
                        Ok(value) => break (text, value),
                        Err(e) => {
                            println!("Bad value: {}", e);
                            initial = text;
                        }
                    }
                };
                editor.add_history_entry(&text);
                save_history(&mut editor,
                    &format!("var_{}", &var_type.type_name()));
                response.send(VarInput::Value(value)).ok();
            }
            Some(Control::ShowHistory { ack } ) => {
                match show_history(editor.history()) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("Error displaying history: {}", e);
                    }
                }
                ack.send(()).ok();
            }
            Some(Control::SpawnEditor { entry, response }) => {
                let h = editor.history();
                let e = entry.unwrap_or(-1);
                let normal = if e < 0 {
                    (h.len() as isize)
                        // last history entry is the current command which
                        // is useless
                        .saturating_sub(1)
                        .saturating_add(e)
                } else {
                    e
                };
                if normal < 0 {
                    eprintln!("No history entry {}", e);
                    response.send(Input::Interrupt).ok();
                    continue;
                }
                let value = if let Some(value) = h.get(normal as usize) {
                    value
                } else {
                    eprintln!("No history entry {}", e);
                    response.send(Input::Interrupt).ok();
                    continue;
                };
                let mut text = match spawn_editor(value) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error editing history entry: {}", e);
                        response.send(Input::Interrupt).ok();
                        continue;
                    }
                };
                text.truncate(text.trim_end().len());
                response.send(Input::Text(text)).ok();
            }
        }
    }
    save_history(&mut editor, "edgeql");
    Ok(())
}

fn show_history(history: &History) -> Result<(), anyhow::Error> {
    let pager = env::var("EDGEDB_PAGER")
        .or_else(|_| env::var("PAGER"))
        .unwrap_or_else(|_| 
            if cfg!(windows) {
                String::from("more.com")
            } else {
                String::from("less -R")
            }
        );
    let mut items = pager.split_whitespace();
    let mut cmd = Command::new(items.next().unwrap());
    cmd.stdin(Stdio::piped());
    cmd.args(items);
    let mut child = cmd.spawn()?;
    let mut childin = child.stdin.take().expect("stdin is piped");
    for index in (0..history.len()).rev() {
        if let Some(s) = history.get(index) {
            let prefix = format!("[-{}] ", history.len() - index);
            let mut lines = s.lines();
            if let Some(first) = lines.next() {
                writeln!(childin, "{}{}", prefix, first)?;
            }
            for next in lines {
                writeln!(childin, "{:1$}{2}", "", prefix.len(), next)?;
            }
        }
    }
    let res = child.wait()?;
    if res.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("pager exited with: {}", res))
    }
}

fn spawn_editor(data: &str) -> Result<String, anyhow::Error> {
    let mut temp_file = tempfile::Builder::new()
        .suffix(".edgeql")
        .tempfile()?;
    temp_file.write_all(data.as_bytes())?;
    let temp_path = temp_file.into_temp_path();
    let editor = editor_path();
    let mut items = editor.split_whitespace();
    let mut cmd = Command::new(items.next().unwrap());
    cmd.args(items);
    cmd.arg(&temp_path);
    let res = cmd.status()?;
    if res.success() {
        Ok(fs::read_to_string(&temp_path)?)
    } else {
        Err(anyhow::anyhow!("editor exited with: {}", res))
    }
}
