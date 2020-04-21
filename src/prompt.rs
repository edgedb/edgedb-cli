use std::borrow::Cow;
use std::fs;
use std::io::{ErrorKind, Write};
use std::env;
use std::process::{Command, Stdio};

use anyhow::{self, Context as _Context};
use async_std::sync::{Sender, Receiver};
use async_std::task;
use dirs::data_local_dir;
use rustyline::{self, error::ReadlineError, KeyPress, Cmd};
use rustyline::{Editor, Config, Helper, Context};
use rustyline::config::EditMode;
use rustyline::hint::Hinter;
use rustyline::highlight::{Highlighter, PromptInfo};
use rustyline::history::History;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::completion::Completer;

use edgeql_parser::preparser::full_statement;
use crate::commands::backslash;
use crate::print::style::{Styler, Style};
use crate::highlight;

use colorful::Colorful;


pub enum Control {
    EdgeqlInput { database: String, initial: String },
    VariableInput { name: String, type_name: String, initial: String },
    ShowHistory,
    SpawnEditor { entry: Option<isize> },
    ViMode,
    EmacsMode,
}

pub enum Input {
    Text(String),
    Eof,
    Interrupt,
}

pub struct EdgeqlHelper {
    styler: Styler,
}

impl Helper for EdgeqlHelper {}
impl Hinter for EdgeqlHelper {
    fn hint(&self, line: &str, pos: usize, _ctx: &Context) -> Option<String> {
        // TODO(tailhook) strip leading whitespace
        // TODO(tailhook) hint argument name if not on the end of line
        if line.starts_with("\\") && pos == line.len() {
            let mut hint = None;
            for item in backslash::HINTS {
                if item.starts_with(line) {
                    if hint.is_some() {
                        // more than one item matches
                        hint = None;
                        break;
                    } else {
                        hint = Some(item);
                    }
                }
            }
            if let Some(hint) = hint {
                return Some(hint[line.len()..].into())
            }
        }
        return None;
    }
}

impl Highlighter for EdgeqlHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self,
        prompt: &'p str, info: PromptInfo<'_>,)
        -> Cow<'b, str>
    {
        if info.line_no() > 0 {
            return format!("{0:.>1$}", " ", prompt.len()).into();
        } else {
            return prompt.into();
        }
    }
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let line_trim = line.trim_start();
        if line_trim.starts_with('\\') {
            let off = line.len() - line_trim.len();
            if let Some(cmd) = line.split_whitespace().next() {
                if backslash::COMMAND_NAMES.contains(&cmd) {
                    let mut buf = String::with_capacity(line.len() + 8);
                    buf.push_str(&line[..off]);
                    self.styler.apply(Style::BackslashCommand, cmd, &mut buf);
                    buf.push_str(&line[off+cmd.len()..]);
                    return buf.into();
                } else if !backslash::COMMAND_NAMES
                    .iter().any(|c| c.starts_with(cmd))
                {
                    let mut buf = String::with_capacity(line.len() + 8);
                    buf.push_str(&line[..off]);
                    self.styler.apply(Style::Error, cmd, &mut buf);
                    buf.push_str(&line[off+cmd.len()..]);
                    return buf.into();
                }
            }
            return line.into();
        } else {
            return highlight::edgeql(line, &self.styler).into();
        }
    }
    fn highlight_char<'l>(&self, _line: &'l str, _pos: usize) -> bool {
        // TODO(tailhook) optimize: only need to return true on insert
        true
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        return hint.light_gray().to_string().into()
    }
}
impl Validator for EdgeqlHelper {
    fn validate(&self, ctx: &mut ValidationContext)
        -> Result<ValidationResult, ReadlineError>
    {
        let line = ctx.input().trim();
        if line.trim().is_empty() {
            return Ok(ValidationResult::Valid(None));
        }
        if line.starts_with("\\") {
            match backslash::parse(line) {
                Ok(_) => Ok(ValidationResult::Valid(None)),
                Err(e) => {
                    Ok(ValidationResult::Invalid(Some(
                        format!("  ← {}", e.hint))))
                }
            }
        } else {
            let mut data = ctx.input();
            loop {
                match full_statement(&data.as_bytes(), None) {
                    Ok(bytes) => {
                        data = &data[bytes..];
                        if data.trim().is_empty() {
                            return Ok(ValidationResult::Valid(None))
                        }
                    }
                    Err(_) => {
                        return Ok(ValidationResult::Incomplete)
                    }
                }
            }
        }
    }
}
impl Completer for EdgeqlHelper {
    type Candidate = String;
    fn complete(&self, line: &str, pos: usize, _ctx: &Context)
        -> Result<(usize, Vec<Self::Candidate>), ReadlineError>
    {
        // TODO(tailhook) strip leading whitespace
        // TODO(tailhook) argument completion
        if line.starts_with("\\") && pos == line.len() {
            let mut options = Vec::new();
            for item in backslash::COMMAND_NAMES {
                if item.starts_with(line) {
                    options.push((*item).into());
                }
            }
            return Ok((0, options))
        }
        Ok((pos, Vec::new()))
    }
}

fn load_history<H: rustyline::Helper>(ed: &mut Editor<H>, name: &str)
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

fn save_history<H: Helper>(ed: &mut Editor<H>, name: &str) {
    _save_history(ed, name).map_err(|e| {
        eprintln!("Can't save history: {:#}", e);
    }).ok();
}

pub fn create_editor(mode: EditMode) -> Editor<EdgeqlHelper> {
    let config = Config::builder();
    let config = config.edit_mode(mode);
    let mut editor = Editor::<EdgeqlHelper>::with_config(config.build());
    editor.bind_sequence(KeyPress::Enter, Cmd::AcceptOrInsertLine);
    load_history(&mut editor, "edgeql").map_err(|e| {
        eprintln!("Can't load history: {:#}", e);
    }).ok();
    editor.set_helper(Some(EdgeqlHelper {
        styler: Styler::dark_256(),
    }));
    return editor;
}

pub fn var_editor(mode: EditMode, type_name: &str) -> Editor<()> {
    let config = Config::builder();
    let config = config.edit_mode(mode);
    let mut editor = Editor::<()>::with_config(config.build());
    editor.bind_sequence(KeyPress::Enter, Cmd::AcceptOrInsertLine);
    load_history(&mut editor, &format!("var_{}", type_name)).map_err(|e| {
        eprintln!("Can't load history: {:#}", e);
    }).ok();
    return editor;
}

pub fn edgeql_input(prompt: &mut String, editor: &mut Editor<EdgeqlHelper>,
    data: &Sender<Input>, database: &str, initial: &str)
{
    prompt.clear();
    prompt.push_str(&database);
    prompt.push_str("> ");
    let text = match
        editor.readline_with_initial(&prompt, (&initial, ""))
    {
        Ok(text) => text,
        Err(ReadlineError::Eof) => {
            task::block_on(data.send(Input::Eof));
            return;
        }
        Err(ReadlineError::Interrupted) => {
            task::block_on(data.send(Input::Interrupt));
            return;
        }
        Err(e) => {
            eprintln!("Readline error: {}", e);
            return;
        }
    };
    editor.add_history_entry(&text);
    task::block_on(data.send(Input::Text(text)))
}


pub fn main(data: Sender<Input>, control: Receiver<Control>)
    -> Result<(), anyhow::Error>
{
    let mut mode = EditMode::Emacs;
    let mut editor = create_editor(mode);
    let mut prompt = String::from("> ");
    'outer: loop {
        match task::block_on(control.recv()) {
            None => break 'outer,
            Some(Control::ViMode) => {
                save_history(&mut editor, "edgeql");
                mode = EditMode::Vi;
                editor = create_editor(mode);
            }
            Some(Control::EmacsMode) => {
                save_history(&mut editor, "edgeql");
                mode = EditMode::Emacs;
                editor = create_editor(mode);
            }
            Some(Control::EdgeqlInput { database, initial }) => {
                edgeql_input(&mut prompt, &mut editor, &data,
                    &database, &initial);
            }
            Some(Control::VariableInput { name, type_name, initial })
            => {
                prompt.clear();
                prompt.push_str("Variable <");
                prompt.push_str(&type_name);
                prompt.push_str(">$");
                prompt.push_str(&name);
                prompt.push_str(": ");
                let mut editor = var_editor(mode, &type_name);
                let text = match
                    editor.readline_with_initial(&prompt, (&initial, ""))
                {
                    Ok(text) => text,
                    Err(ReadlineError::Eof) => {
                        task::block_on(data.send(Input::Eof));
                        continue;
                    }
                    Err(ReadlineError::Interrupted) => {
                        task::block_on(data.send(Input::Interrupt));
                        continue;
                    }
                    Err(e) => Err(e)?,
                };
                editor.add_history_entry(&text);
                save_history(&mut editor, &format!("var_{}", &type_name));
                task::block_on(data.send(Input::Text(text)))
            }
            Some(Control::ShowHistory) => {
                match show_history(editor.history()) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("Error displaying history: {}", e);
                    }
                }
            }
            Some(Control::SpawnEditor { entry }) => {
                let h = editor.history();
                let e = entry.unwrap_or(-1);
                let normal = if e < 0 {
                    (h.len() as isize)
                        // last history entry is the current command which
                        // is useless
                        .saturating_sub(1)
                        .saturating_add(e)
                } else {
                    e as isize
                };
                if normal < 0 {
                    eprintln!("No history entry {}", e);
                    task::block_on(data.send(Input::Interrupt));
                    continue;
                }
                let value = if let Some(value) = h.get(normal as usize) {
                    value
                } else {
                    eprintln!("No history entry {}", e);
                    task::block_on(data.send(Input::Interrupt));
                    continue;
                };
                let mut text = match spawn_editor(value) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error editing history entry: {}", e);
                        task::block_on(data.send(Input::Interrupt));
                        continue;
                    }
                };
                text.truncate(text.trim_end().len());
                task::block_on(data.send(Input::Text(text)));
            }
        }
    }
    save_history(&mut editor, "edgeql");
    Ok(())
}

fn show_history(history: &History) -> Result<(), anyhow::Error> {
    let pager = env::var("EDGEDB_PAGER")
        .or_else(|_| env::var("PAGER"))
        .unwrap_or_else(|_| String::from("less -RF"));
    let mut items = pager.split_whitespace();
    let mut cmd = Command::new(items.next().unwrap());
    cmd.stdin(Stdio::piped());
    cmd.args(items);
    let mut child = cmd.spawn()?;
    let childin = child.stdin.as_mut().expect("stdin is piped");
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
    drop(childin);
    let res = child.wait()?;
    if res.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("pager exited with: {}", res))
    }
}

fn spawn_editor(data: &str) -> Result<String, anyhow::Error> {
    let mut temp_file = tempfile::Builder::new()
        .suffix(".edgedb")
        .tempfile()?;
    temp_file.write_all(data.as_bytes())?;
    let temp_path = temp_file.into_temp_path();
    let editor = env::var("EDGEDB_EDITOR")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| String::from("vim"));
    let mut items = editor.split_whitespace();
    let mut cmd = Command::new(items.next().unwrap());
    cmd.args(items);
    cmd.arg(&temp_path);
    let res = cmd.status()?;
    if res.success() {
        return Ok(fs::read_to_string(&temp_path)?);
    } else {
        Err(anyhow::anyhow!("editor exited with: {}", res))
    }
}

