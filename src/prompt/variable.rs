use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use colorful::Colorful;
use edgedb_protocol::value::Value;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::{Helper, Context};


#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", description)]
    Mistake { offset: Option<usize>, description: String },
}

fn no_pos_err<E: fmt::Display>(err: E) -> Error {
    Error::Mistake { offset: None, description: err.to_string() }
}

pub trait VariableInput: fmt::Debug + Send + Sync + 'static {
    fn parse(&self, input: &str) -> Result<Value, Error>;
    fn type_name(&self) -> &str;
}

#[derive(Debug)]
pub struct Str;

impl VariableInput for Str {
    fn type_name(&self) -> &str { "str" }
    fn parse(&self, input: &str) -> Result<Value, Error> {
        Ok(Value::Str(input.into()))
    }
}

#[derive(Debug)]
pub struct Uuid;

impl VariableInput for Uuid {
    fn type_name(&self) -> &str { "uuid" }
    fn parse(&self, input: &str) -> Result<Value, Error> {
        Ok(Value::Uuid(input.parse().map_err(no_pos_err)?))
    }
}

#[derive(Debug)]
pub struct Int16;

impl VariableInput for Int16 {
    fn type_name(&self) -> &str { "int16" }
    fn parse(&self, input: &str) -> Result<Value, Error> {
        Ok(Value::Int16(input.parse().map_err(no_pos_err)?))
    }
}

#[derive(Debug)]
pub struct Int32;

impl VariableInput for Int32 {
    fn type_name(&self) -> &str { "int32" }
    fn parse(&self, input: &str) -> Result<Value, Error> {
        Ok(Value::Int32(input.parse().map_err(no_pos_err)?))
    }
}

#[derive(Debug)]
pub struct Int64;

impl VariableInput for Int64 {
    fn type_name(&self) -> &str { "int64" }
    fn parse(&self, input: &str) -> Result<Value, Error> {
        Ok(Value::Int64(input.parse().map_err(no_pos_err)?))
    }
}

pub struct VarHelper {
    var_type: Arc<dyn VariableInput>,
}

pub struct ErrorHint(String);

impl rustyline::hint::Hint for ErrorHint {
    fn completion(&self) -> Option<&str> { None }
    fn display(&self) -> &str { self.0.as_ref() }
}

impl VarHelper {
    pub fn new(var_type: Arc<dyn VariableInput>) -> VarHelper {
        VarHelper {
            var_type,
        }
    }
}

impl Helper for VarHelper {}
impl Hinter for VarHelper {
    type Hint = ErrorHint;
    fn hint(&self, line: &str, _pos: usize, _ctx: &Context)
        -> Option<Self::Hint>
    {
        if line == "" {  // be friendly from the start
            return None;
        }
        match self.var_type.parse(line) {
            Ok(_) => None,
            Err(e) => Some(ErrorHint(format!(" -- {}", e))),
        }
    }
}

impl Highlighter for VarHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        match self.var_type.parse(line) {
            Ok(_) => line.into(),
            Err(_) => line.light_red().to_string().into(),
        }
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        return hint.light_gray().to_string().into()
    }
    fn highlight_char<'l>(&self, _line: &'l str, _pos: usize) -> bool {
        // needed to highlight hint
        true
    }
    fn has_continuation_prompt(&self) -> bool {
        true
    }
}

impl Validator for VarHelper {
    fn validate(&self, ctx: &mut ValidationContext)
        -> Result<ValidationResult, ReadlineError>
    {
        match self.var_type.parse(ctx.input()) {
            Ok(_) => Ok(ValidationResult::Valid(None)),
            Err(e) => Ok(ValidationResult::Invalid(
                Some(format!(" -- {}", e))
            )),
        }
    }
}

impl Completer for VarHelper {
    type Candidate = String;
    fn complete(&self, _line: &str, pos: usize, _ctx: &Context)
        -> Result<(usize, Vec<Self::Candidate>), ReadlineError>
    {
        Ok((pos, Vec::new()))
    }
}
