use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use std::convert::TryInto;
use std::fmt::format;
use std::rc::Rc;
use std::str::FromStr;
use anyhow::Context as _;

use colorful::Colorful;
use bigdecimal::BigDecimal;
use nom::combinator::{map_opt, value};
use nom::combinator::{cut, fail, map, map_res};
use edgedb_protocol::value::Value;
use edgedb_protocol::model;
use nom::bytes::complete::{escaped, tag, take, take_till, take_until, take_while};
use nom::character::complete::{alphanumeric1, char, i16, i32, i64, one_of};
use nom::{IResult, Needed, Parser};
use nom::branch::alt;
use nom::character::{is_alphabetic, is_alphanumeric};
use nom::Err::{Error, Failure, Incomplete};
use nom::error::convert_error;
use nom::multi::separated_list0;
use nom::number::complete::{double, f32, float, recognize_float_parts};
use nom::sequence::{preceded, terminated};
use num_bigint::ToBigInt;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::{Helper, Context};

pub trait VariableInput: fmt::Debug + Send + Sync + 'static {
    fn parse<'a >(&self, input: &'a str) -> IResult<&'a str, Value>;
    fn type_name(&self) -> &str;
}

fn space(i: &str) -> IResult<&str, &str> {
    let chars = " \t\r\n";

    // nom combinators like `take_while` return a function. That function is the
    // parser,to which we can pass the input
    take_while(move |c| chars.contains(c))(i)
}

fn quoted_str_parser(input: &str) -> IResult<&str, &str> {
    preceded(
        char('\"'),
        cut(
            terminated(
                escaped(
                    alphanumeric1,
                    '\\',
                    one_of("\"n\\")),
                char('\"')
            )
        )
    )(input)
}

#[derive(Debug)]
pub struct Str;

impl VariableInput for Str {
    fn type_name(&self) -> &str { "str" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(
            quoted_str_parser,
            |v| Value::Str(String::from(v))
        )(input)
    }
}

#[derive(Debug)]
pub struct Uuid;

impl VariableInput for Uuid {
    fn type_name(&self) -> &str { "uuid" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        fn uuid_parser(i: &str) ->  IResult<&str, &str> {
            let remaining_ref = Rc::new(RefCell::new(32));
            let valid_ref = Rc::new(RefCell::new(true));

            let result = take_till(|c: char| -> bool {
                if c.is_alphanumeric() {
                    *remaining_ref.borrow_mut() -= 1;
                } else if c != '-' {
                    *valid_ref.borrow_mut() = false;
                    return true;
                }

                if *remaining_ref.borrow_mut() <= 0 {
                    return true;
                }

                return false;
            })(i);

            if !*valid_ref.borrow_mut() || *remaining_ref.borrow_mut() > 0 {
                return fail(i)
            }

            return result
        }

        map(
            map_res(uuid_parser, |b: &str| b.parse::<uuid::Uuid>().context(format!("cannot parse {} into a uuid", b))),
            |v| Value::Uuid(v)
        )(input)
    }
}

#[derive(Debug)]
pub struct Int16;

impl VariableInput for Int16 {
    fn type_name(&self) -> &str { "int16" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(i16, Value::Int16)(input)
    }
}

#[derive(Debug)]
pub struct Int32;

impl VariableInput for Int32 {
    fn type_name(&self) -> &str { "int32" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(i32, Value::Int32)(input)
        //Ok(Value::Int32(input.parse().map_err(no_pos_err)?))
    }
}

#[derive(Debug)]
pub struct Int64;

impl VariableInput for Int64 {
    fn type_name(&self) -> &str { "int64" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(i64, Value::Int64)(input)
        //Ok(Value::Int64(input.parse().map_err(no_pos_err)?))
    }
}

#[derive(Debug)]
pub struct Float32;

impl VariableInput for Float32 {
    fn type_name(&self) -> &str { "float32" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(float, Value::Float32)(input)
    }
}

#[derive(Debug)]
pub struct Float64;

impl VariableInput for Float64 {
    fn type_name(&self) -> &str { "float64" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(double, Value::Float64)(input)
    }
}

#[derive(Debug)]
pub struct Bool;

impl VariableInput for Bool {
    fn type_name(&self) -> &str { "bool" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        alt((
            value(Value::Bool(true), tag("true")),
            value(Value::Bool(false), tag("false"))
        ))(input)
    }
}

#[derive(Debug)]
pub struct BigInt;

impl VariableInput for BigInt {
    fn type_name(&self) -> &str { "bigint" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map_res(
            take_while(|c: char| c.is_digit(10)),
            |v: &str| -> Result<Value, anyhow::Error> {
                let dec: BigDecimal = v.parse()?;
                let int = dec.to_bigint()
                    .context("number is not an integer")?;
                let int = int.try_into()?;
                Ok(Value::BigInt(int))
            }
        )(input)
    }
}

#[derive(Debug)]
pub struct Decimal;

impl VariableInput for Decimal {
    fn type_name(&self) -> &str { "decimal" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(
            map_res(
                map_res(
                    recognize_float_parts,
                    |v| {
                        // TODO: could be done better?
                        let mut fmt = "".to_owned();

                        if !v.0 {
                            fmt.push_str("-")
                        }

                        fmt.push_str(v.1);

                        if v.2 != "" {
                            fmt.push('.');
                            fmt.push_str(v.2);
                        }

                        if v.3 != 0{
                            fmt.push('e');
                            fmt.push_str(&v.3.to_string());
                        }

                        BigDecimal::from_str(&fmt)
                    }
                ),
                |v| v.try_into()
            ),
            |v| Value::Decimal(v)
        )(input)
    }
}

#[derive(Debug)]
pub struct Json;

impl VariableInput for Json {
    fn type_name(&self) -> &str { "json" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        // treat as string, and then validate the strings content
        map_res(
            quoted_str_parser,
            |v| -> Result<Value, anyhow::Error> {
                let j = serde_json::from_str::<serde_json::Value>(v)?;

                Ok(Value::Json(model::Json::new_unchecked(v.into())))
            }
        )(input)
    }
}

#[derive(Debug)]
pub struct Array {
    pub element_type: Arc<dyn VariableInput>
}

impl VariableInput for Array {
    fn type_name(&self) -> &str { "array" }
    fn parse<'a>(&self, input: &'a str) -> IResult<&'a str, Value> {
        map(
            preceded(
                char('['),
                cut(
                    terminated(
                        separated_list0(
                            preceded(
                                space,
                                char(',')
                            ),
                            |s| self.element_type.parse(s)),
                        preceded(
                            space,
                            char(']')
                        ),
                    )
                ),
            ),
            |v| Value::Array(v)
        )(input)
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
            Err(e) => {
                Some(ErrorHint(format!(" -- {}", match e {
                    Failure(f) => format!("Fail: Caused by {} at {}", f.code.description(), f.input),
                    Incomplete(v) => format!(
                        "Incomplete entry{}",
                        match v {
                            Needed::Unknown => "".to_string(),
                            Needed::Size(s) => format!(", need {s} more chars")
                        }
                    ),
                    nom::Err::Error(e) => format!("Err: Caused by {} at {}", e.code.description(), e.input),
                })))
            }
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
        return hint.rgb(0x56, 0x56, 0x56).to_string().into()
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
            //Err(Error::Incompil) => Ok(ValidationResult::Incomplete),
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
