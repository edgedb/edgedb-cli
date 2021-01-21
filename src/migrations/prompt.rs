use std::borrow::Cow;

use anyhow::Context as _;
use colorful::Colorful;
use rustyline::{self, error::ReadlineError, KeyPress, Cmd};
use rustyline::{Editor, Config, Helper};
use rustyline::config::EditMode;
use rustyline::hint::Hinter;
use rustyline::highlight::{Highlighter, PromptInfo};
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::completion::Completer;
use edgeql_parser::expr;

use crate::highlight;
use crate::print::style::Styler;
use crate::prompt::{load_history, save_history};


pub struct ExpressionHelper {
    styler: Styler,
}
impl Helper for ExpressionHelper {}
impl Hinter for ExpressionHelper {
    type Hint = String;
}
impl Highlighter for ExpressionHelper {
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
        let mut buf = String::with_capacity(line.len() + 8);
        highlight::edgeql(&mut buf, line, &self.styler);
        return buf.into();
    }
    fn highlight_char<'l>(&self, _line: &'l str, _pos: usize) -> bool {
        // TODO(tailhook) optimize: only need to return true on insert
        true
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        return hint.light_gray().to_string().into()
    }
    fn has_continuation_prompt(&self) -> bool {
        true
    }
}

impl Validator for ExpressionHelper {
    fn validate(&self, ctx: &mut ValidationContext)
        -> Result<ValidationResult, ReadlineError>
    {
        match expr::check(ctx.input()) {
            Ok(()) => Ok(ValidationResult::Valid(None)),
            Err(expr::Error::MissingBracket {..})
            | Err(expr::Error::Empty)
            => Ok(ValidationResult::Incomplete),
            Err(e) => Ok(ValidationResult::Invalid(Some(e.to_string()))),
        }
    }
}

impl Completer for ExpressionHelper {
    type Candidate = String;
}

pub fn expression(prompt: &str, history_name: &str)
    -> Result<String, anyhow::Error>
{
    let history_name = format!("migr_{}", &history_name);
    let config = Config::builder();
    let config = config.edit_mode(EditMode::Emacs);
    let mut editor = Editor::<ExpressionHelper>::with_config(
        config.clone().build());
    editor.bind_sequence(KeyPress::Enter,
        Cmd::AcceptOrInsertLine { accept_in_the_middle: false });
    editor.bind_sequence(KeyPress::Meta('\r'), Cmd::AcceptLine);
    load_history(&mut editor, &history_name).map_err(|e| {
        eprintln!("Can't load history: {:#}", e);
    }).ok();
    editor.set_helper(Some(ExpressionHelper {
        styler: Styler::dark_256(),
    }));
    let text = editor.readline(&prompt).context("readline error")?;
    editor.add_history_entry(&text);
    save_history(&mut editor, &history_name);
    Ok(text)
}
