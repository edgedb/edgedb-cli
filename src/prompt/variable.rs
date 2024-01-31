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
use colorful::core::StrMarker;
use combine::parser::combinator::and_then;
use combine::stream::Range;
use nom::combinator::{map_opt, not, success, value, verify};
use nom::combinator::{cut, fail, map, map_res};
use edgedb_protocol::value::Value;
use edgedb_protocol::model;
use nom::bytes::complete::{escaped, is_not, tag, take, take_till, take_until, take_while, take_while_m_n};
use nom::character::complete::{alphanumeric1, anychar, char, i16, i32, i64, multispace0, multispace1, one_of};
use nom::{error, IResult, Needed, Parser};
use nom::branch::alt;
use nom::character::{is_alphabetic, is_alphanumeric};
use nom::Err::{Error, Failure, Incomplete};
use nom::error::{context, ContextError, convert_error, ErrorKind, FromExternalError, ParseError};
use nom::multi::{fold_many0, separated_list0};
use nom::number::complete::{double, f32, float, recognize_float_parts};
use nom::sequence::{delimited, preceded, terminated, tuple};
use num_bigint::ToBigInt;
use rustyline::completion::Completer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationResult, ValidationContext};
use rustyline::{Helper, Context};

type ParseResult<'a, I = &'a str, R = Value> = IResult<I, R, ParsingError>;

#[derive(Debug, thiserror::Error)]
pub enum ParsingError {
    #[error("{}", description)]
    Mistake { kind: Option<ErrorKind>, description: String },
    #[error("External error occurred: {}", error)]
    External { kind: ErrorKind, description: String, error: anyhow::Error },
    #[error("value is incomplete")]
    Incomplete,
}

impl ParseError<&str> for ParsingError {
    // on one line, we show the error code and the input that caused it
    fn from_error_kind(input: &str, kind: ErrorKind) -> Self {
        let message = format!("{:?} failed on the following input: \'{:?}\'", kind.description(), input);
        ParsingError::Mistake { kind: Some(kind), description: message }
    }

    // if combining multiple errors, we show them one after the other
    fn append(input: &str, kind: ErrorKind, other: Self) -> Self {
        let message = format!("{}, then: {}", other, ParsingError::from_error_kind(input, kind));
        ParsingError::Mistake { kind: Some(kind), description: message }
    }

    fn from_char(input: &str, c: char) -> Self {

        let message = if input != "" {
            format!("Expected '{}' in {:?}", c, input)
        } else {
            format!("Expected '{}'", c)
        };
        ParsingError::Mistake { kind: None, description: message }
    }

    fn or(self, other: Self) -> Self {
        let message = format!("{}, or: {}", self, other);
        ParsingError::Mistake { kind: None, description: message }
    }
}

impl ContextError<&str> for ParsingError {
    fn add_context(_input: &str, ctx: &'static str, other: Self) -> Self {
        let message = format!("{} -> {}", ctx, other);
        ParsingError::Mistake { kind: None, description: message }
    }
}

impl FromExternalError<&str, anyhow::Error> for ParsingError {
    fn from_external_error(input: &str, kind: ErrorKind, e: anyhow::Error) -> Self {
        ParsingError::External {
            error: e,
            kind,
            description: format!("Failed at '{}'", input)
        }
    }
}

#[repr(u8)]
pub enum InputFlags {
    None,
    ForceQuotedStrings
}

pub trait VariableInput: fmt::Debug + Send + Sync + 'static {
    fn type_name(&self) -> &str;
    fn parse<'a >(&self, input: &'a str, flags: InputFlags) -> ParseResult<'a>;
}

fn white_space<'a, O, E: ParseError<&'a str>, F: Parser<&'a str, O, E>>(
    f: F,
) -> impl Parser<&'a str, O, E> {
    delimited(multispace0, f, multispace0)
}
fn space(i: &str) -> IResult<&str, &str, ParsingError> {
    let chars = " \t\r\n";
    take_while(move |c| chars.contains(c))(i)
}

fn quoted_str(input: &str) -> IResult<&str, String, ParsingError> {
    context(
        "any_quote_str",
        alt((
            single_quoted_str,
            double_quoted_str
        ))
    )(input)
}

fn single_quoted_str(input: &str) -> IResult<&str, String, ParsingError> {
    context(
        "single_quote_str",
        |s| quoted_str_parser(s, '\'', "\'\\")
    )(input)
}

fn double_quoted_str(input: &str) -> IResult<&str, String, ParsingError> {
    context(
        "double_quote_str",
        |s| quoted_str_parser(s, '\"', "\"\\")
    )(input)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StringFragment<'a> {
    Literal(&'a str),
    EscapedChar(char),
    EscapedWS,
}

// heavily based on https://github.com/rust-bakery/nom/blob/main/examples/string.rs
fn quoted_str_parser<'a>(input: &'a str, quote: char, esc: &str) -> IResult<&'a str, String, ParsingError> {
    context(
        "quoted_string",
        delimited(
            char(quote),
            fold_many0(
                alt((
                    map(
                        verify(is_not(esc), |s: &str| !s.is_empty()),
                        StringFragment::Literal
                    ),
                    map(
                        preceded(
                            char('\\'),
                            alt((
                                map_opt(
                                    map_res(
                                        preceded(
                                            char('u'),
                                            delimited(
                                                char('{'),
                                                take_while_m_n(1,6, |c: char| c.is_ascii_hexdigit()),
                                                char('}')
                                            )
                                        ),
                                        move |hex| u32::from_str_radix(hex, 16).context("Failed to parse hex digit")
                                    ),
                                    std::char::from_u32
                                ),
                                value('\n', char('n')),
                                value('\r', char('r')),
                                value('\t', char('t')),
                                value('\u{08}', char('b')),
                                value('\u{0C}', char('f')),
                                value('\\', char('\\')),
                                value('/', char('/')),
                                value('"', char('"')),
                                value('\'', char('\'')),
                            ))
                        ),
                        StringFragment::EscapedChar
                    ),
                    value(
                        StringFragment::EscapedWS,
                        preceded(char('\\'), nom::character::streaming::multispace1)
                    ),
                )),
                String::new,
                |mut string, fragment| {
                    match fragment {
                        StringFragment::Literal(s) => string.push_str(s),
                        StringFragment::EscapedChar(c) => string.push(c),
                        StringFragment::EscapedWS => {}
                    }
                    string
                }
            ),
            char(quote),
        )
    )(input)
}

#[derive(Debug)]
pub struct Str;

impl VariableInput for Str {
    fn type_name(&self) -> &str { "str" }
    fn parse<'a>(&self, input: &'a str, flags: InputFlags) -> ParseResult<'a> {
        match flags {
            InputFlags::ForceQuotedStrings => {
                context(
                    "str",
                    map(
                        quoted_str,
                        Value::Str
                    )
                )(input)
            }
            _ => context(
                "str",
                |s: &str| Ok(("", Value::Str(s.to_string())))
            )(input)
        }
    }
}

#[derive(Debug)]
pub struct Uuid;

impl VariableInput for Uuid {
    fn type_name(&self) -> &str { "uuid" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        fn no_dashes(i: &str) -> ParseResult<&str, uuid::Uuid> {
            map_res(
                nom::bytes::streaming::take_while_m_n(
                    32usize,
                    32usize,
                    char::is_alphanumeric,
                ),
                |s| uuid::Uuid::from_str(s).context("Cannot parse to UUID")
            )(i)
        }

        fn dashes(i: &str) -> ParseResult<&str, uuid::Uuid> {
            map_res(
                tuple((
                    nom::bytes::streaming::take_while_m_n(
                        8,
                        8,
                        char::is_alphanumeric,
                    ),
                    char('-'),
                    nom::bytes::streaming::take_while_m_n(
                        4,
                        4,
                        char::is_alphanumeric,
                    ),
                    char('-'),
                    nom::bytes::streaming::take_while_m_n(
                        4,
                        4,
                        char::is_alphanumeric,
                    ),
                    char('-'),
                    nom::bytes::streaming::take_while_m_n(
                        4,
                        4,
                        char::is_alphanumeric,
                    ),
                    char('-'),
                    nom::bytes::streaming::take_while_m_n(
                        12,
                        12,
                        char::is_alphanumeric,
                    ),
                )),
                |s| uuid::Uuid::from_str(&[s.0, s.2, s.4, s.6, s.8].join("")).context("UUID format is malformed")
            )(i)
        }

        context(
            "uuid",
            map(
                alt((
                    no_dashes,
                    dashes,
                )),
                |v| Value::Uuid(v)
            )
        )(input)
    }
}

#[derive(Debug)]
pub struct Int16;

impl VariableInput for Int16 {
    fn type_name(&self) -> &str { "int16" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context("int16", map(i16, Value::Int16))(input)
    }
}

#[derive(Debug)]
pub struct Int32;

impl VariableInput for Int32 {
    fn type_name(&self) -> &str { "int32" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "int32",
            map(i32, Value::Int32)
        )(input)
    }
}

#[derive(Debug)]
pub struct Int64;

impl VariableInput for Int64 {
    fn type_name(&self) -> &str { "int64" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "int64",
            map(i64, Value::Int64)
        )(input)
    }
}

#[derive(Debug)]
pub struct Float32;

impl VariableInput for Float32 {
    fn type_name(&self) -> &str { "float32" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context("float32", map(float, Value::Float32))(input)
    }
}

#[derive(Debug)]
pub struct Float64;

impl VariableInput for Float64 {
    fn type_name(&self) -> &str { "float64" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context("float64", map(double, Value::Float64))(input)
    }
}

#[derive(Debug)]
pub struct Bool;

impl VariableInput for Bool {
    fn type_name(&self) -> &str { "bool" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "bool",
            alt((
                value(Value::Bool(true), tag("true")),
                value(Value::Bool(false), tag("false"))
            ))
        )(input)
    }
}

#[derive(Debug)]
pub struct BigInt;

impl VariableInput for BigInt {
    fn type_name(&self) -> &str { "bigint" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "bigint",
            map_res(
                take_while(|c: char| c.is_digit(10)),
                |v: &str| -> Result<Value, anyhow::Error> {
                    let dec: BigDecimal = v.parse()?;
                    let int = dec.to_bigint()
                        .context("number is not an integer")?;
                    let int = int.try_into()?;
                    Ok(Value::BigInt(int))
                }
            )
        )(input)
    }
}

#[derive(Debug)]
pub struct Decimal;

impl VariableInput for Decimal {
    fn type_name(&self) -> &str { "decimal" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "decimal",
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

                            BigDecimal::from_str(&fmt).context("format doesn't represent a big decimal")
                        }
                    ),
                    |v| v.try_into().context("BigDecimal cannot be interpolated")
                ),
                |v| Value::Decimal(v)
            )
        )(input)
    }
}

#[derive(Debug)]
pub struct Json;

impl VariableInput for Json {
    fn type_name(&self) -> &str { "json" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        // treat as string, and then validate the strings content
        context(
            "json",
            map_res(
                double_quoted_str,
                |v| -> Result<Value, anyhow::Error> {
                    serde_json::from_str::<serde_json::Value>(&v)?;

                    Ok(Value::Json(model::Json::new_unchecked(v.into())))
                }
            )
        )(input)
    }
}

#[derive(Debug)]
pub struct Array {
    pub element_type: Arc<dyn VariableInput>
}

impl VariableInput for Array {
    fn type_name(&self) -> &str { "array" }
    fn parse<'a>(&self, input: &'a str, _flags: InputFlags) -> ParseResult<'a> {
        context(
            "array",
            map(
                preceded(
                    char('['),
                    cut(
                        terminated(
                            separated_list0(
                                white_space(char(',')),
                                |s| self.element_type.parse(s, InputFlags::ForceQuotedStrings)
                            ),
                            preceded(
                                space,
                                char(']')
                            ),
                        )
                    ),
                ),
                |v| Value::Array(v)
            )
        )(input)
    }
}

pub struct VarHelper {
    var_type: Arc<dyn VariableInput>,
}

pub struct ErrorHint(String);

impl rustyline::hint::Hint for ErrorHint {
    fn display(&self) -> &str { self.0.as_ref() }
    fn completion(&self) -> Option<&str> { None }
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
        match self.var_type.parse(line, InputFlags::None) {
            Ok(r) => {
                if r.0.len() == 0 {
                    return None
                }

                return Some(ErrorHint(" -- excess unparsed content".to_string()))
            },
            Err(e) => {
                Some(ErrorHint(format!(" -- {}", match e {
                    Error(p) | Failure(p) => match p {
                        ParsingError::Mistake {
                            kind,
                            description
                        } => format!("{}: {}", kind.or(Some(ErrorKind::Fail)).unwrap().description(), description),
                        ParsingError::External {
                            description,
                            error,
                            kind: _
                        } => format!("External error occurred: {} {}", description, error),
                        ParsingError::Incomplete => "Incomplete input".to_string(),
                    },
                    Incomplete(Needed::Size(sz)) => format!("Incomplete input, needing {} more chars", sz),
                    Incomplete(_n) => "Incomplete input".to_string(),
                })))
            }
        }
    }
}

impl Highlighter for VarHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        match self.var_type.parse(line, InputFlags::None) {
            Ok(r) => {
                if r.0.len() == 0 {
                    return line.into()
                }

                let mut str = line[..(line.len() - r.0.len())].to_string();
                str.push_str(&r.0.light_red().to_string());
                str.into()
            },
            Err(_) => line.light_red().to_string().into(),
        }
    }
    fn has_continuation_prompt(&self) -> bool {
        true
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        return hint.rgb(0x56, 0x56, 0x56).to_string().into()
    }
    fn highlight_char<'l>(&self, _line: &'l str, _pos: usize) -> bool {
        // needed to highlight hint
        true
    }
}

impl Validator for VarHelper {
    fn validate(&self, ctx: &mut ValidationContext)
        -> Result<ValidationResult, ReadlineError>
    {
        match self.var_type.parse(ctx.input(), InputFlags::None) {
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
