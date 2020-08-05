use std::marker::PhantomData;

use combine::{StreamOnce, Parser, ParseResult};
use combine::{satisfy, between, many, skip_many, eof, choice, opaque};
use combine::parser::combinator::no_partial;
use combine::easy::{Error, Errors, Info};
use combine::error::Tracked;
use edgeql_parser::position::Pos;
use edgeql_parser::tokenizer::{TokenStream, Kind, Token};
use edgeql_parser::helpers::{unquote_string, UnquoteError};

use crate::migrations::migration::Migration;


#[derive(Debug, Clone)]
pub struct Value<'a> {
    kind: Kind,
    value: &'static str,
    phantom: PhantomData<&'a u8>,
}

#[derive(Debug, Clone)]
pub struct TokenMatch<'a> {
    kind: Kind,
    phantom: PhantomData<&'a u8>,
}

pub fn kw<'s>(value: &'static str)
    -> impl Parser<TokenStream<'s>, Output=()>
{
    Value { kind: Kind::Keyword, value, phantom: PhantomData }
    .map(|_| ())
}

pub fn ident<'s>(value: &'static str)
    -> impl Parser<TokenStream<'s>, Output=()>
{
    Value { kind: Kind::Ident, value, phantom: PhantomData }
    .map(|_| ())
}

pub fn kind<'x>(kind: Kind) -> TokenMatch<'x> {
    TokenMatch {
        kind: kind,
        phantom: PhantomData,
    }
}

impl<'a> Parser<TokenStream<'a>> for Value<'a> {
    type Output = Token<'a>;
    type PartialState = ();

    #[inline]
    fn parse_lazy(&mut self, input: &mut TokenStream<'a>)
        -> ParseResult<Self::Output, Errors<Token<'a>, Token<'a>, Pos>>
    {
        satisfy(|c: Token<'a>| {
            c.kind == self.kind && c.value.eq_ignore_ascii_case(self.value)
        }).parse_lazy(input)
    }

    fn add_error(&mut self,
        error: &mut Tracked<<TokenStream<'a> as StreamOnce>::Error>)
    {
        error.error.add_error(Error::Expected(Info::Static(self.value)));
    }
}

impl<'a> Parser<TokenStream<'a>> for TokenMatch<'a> {
    type Output = Token<'a>;
    type PartialState = ();

    #[inline]
    fn parse_lazy(&mut self, input: &mut TokenStream<'a>)
        -> ParseResult<Self::Output, Errors<Token<'a>, Token<'a>, Pos>>
    {
        satisfy(|c: Token<'a>| c.kind == self.kind).parse_lazy(input)
    }

    fn add_error(&mut self,
        error: &mut Tracked<Errors<Token<'a>, Token<'a>, Pos>>)
    {
        error.error.add_error(Error::Expected(Info::Owned(
            format!("{:?}", self.kind))));
    }
}

enum Statement {
    SetMessage(String),
    SetId(String),
    SetParentId(String),
    Ignored,
}

fn chosen_statements<'a>()
    -> impl Parser<TokenStream<'a>, Output=Statement>
{
    use Statement::*;
    kw("SET").with(
        choice((
            ident("id").skip(kind(Kind::Assign))
                .with(kind(Kind::Str))
                .skip(kind(Kind::Semicolon))
                .and_then(|value| -> Result<_, UnquoteError> {
                    Ok(SetId(unquote_string(value.value)?.into()))
                }),
            ident("parent_id").skip(kind(Kind::Assign))
                .with(kind(Kind::Str))
                .skip(kind(Kind::Semicolon))
                .and_then(|value| -> Result<_, UnquoteError> {
                    Ok(SetParentId(unquote_string(value.value)?.into()))
                }),
            ident("message").skip(kind(Kind::Assign))
                .with(kind(Kind::Str))
                .skip(kind(Kind::Semicolon))
                .and_then(|value| -> Result<_, UnquoteError> {
                    Ok(SetMessage(unquote_string(value.value)?.into()))
                }),
            any_statement(),
        ))
    )
}

fn braces<'a>() -> impl Parser<TokenStream<'a>, Output=Statement> {
    use Statement::*;
    opaque!(
        no_partial(between(kind(Kind::OpenBrace), kind(Kind::CloseBrace),
            skip_many(
                satisfy(|t: Token<'a>| !matches!(t.kind, Kind::CloseBrace))
                .map(|_| Ignored)
                .or(braces())))
        .map(|_| Ignored))
    )
}

fn any_statement<'a>()
    -> impl Parser<TokenStream<'a>, Output=Statement>
{
    use Statement::*;

    skip_many(
        satisfy(|t: Token<'a>| {
            !matches!(t.kind, Kind::Semicolon|Kind::CloseBrace|Kind::OpenBrace)
        })
        .map(|_| Ignored)
        .or(braces())
    ).skip(kind(Kind::Semicolon))
    .map(|_| Ignored)
}

fn statement<'a>()
    -> impl Parser<TokenStream<'a>, Output=Statement>
{
    chosen_statements()
    .or(any_statement())
}

fn migration<'a>()
    -> impl Parser<TokenStream<'a>, Output=Migration>
{
    use Statement::*;

    kw("CREATE").and(ident("MIGRATION"))
        .with(between(kind(Kind::OpenBrace), kind(Kind::CloseBrace),
            many(statement())))
        .skip(kind(Kind::Semicolon))
        .skip(eof())
    .and_then(|statements: Vec<_>| -> Result<_, Error<Token<'_>, Token<'_>>> {
        let mut m = Migration {
            message: None,
            id: None,
            parent_id: None,
        };
        for item in statements {
            match item {
                SetId(id) => {
                    if m.id.is_some() {
                        return Err(Error::Message(
                            "duplicate `SET id` statement".into()))?;
                    }
                    m.id = Some(id);
                }
                SetParentId(id) => {
                    if m.parent_id.is_some() {
                        return Err(Error::Message(
                            "duplicate `SET parent_id` statement".into()))?;
                    }
                    m.parent_id = Some(id);
                }
                SetMessage(text) => {
                    if m.message.is_some() {
                        return Err(Error::Message(
                            "duplicate `SET message` statement".into()))?;
                    }
                    m.message = Some(text);
                }
                Ignored => {}
            }
        }
        Ok(m)
    })
}

pub fn parse_migration(data: &str) -> anyhow::Result<Migration> {
    let mut tokens = TokenStream::new(data);
    match migration().parse_stream(&mut tokens) {
        ParseResult::CommitOk(res) => Ok(res),
        ParseResult::PeekOk(_) => unreachable!(),
        ParseResult::CommitErr(e) => anyhow::bail!("parse error: {}", e),
        ParseResult::PeekErr(e) => anyhow::bail!("parse error: {:?}", e),
    }
/*
            match (&mut tokens).next() {
                Some(Ok(t)) => {
                    anyhow::bail!("end of file expected, got '{}'", t.token)
                }
                Some(Err(e)) => {
                    anyhow::bail!(
                        "end of file expected, got parse error: {:?}", e)
                }
                None => {}
            }
            */
}

#[cfg(test)]
mod test {
    use super::parse_migration;

    #[test]
    fn empty() {
        let m = parse_migration("CREATE MIGRATION {};").unwrap();
        assert_eq!(m.id, None);
        assert_eq!(m.message, None);
        assert_eq!(m.parent_id, None);
    }

    #[test]
    fn set_id() {
        let m = parse_migration("CREATE MIGRATION { set id := 'u123';};")
            .unwrap();
        assert_eq!(m.id, Some("u123".into()));
        assert_eq!(m.message, None);
        assert_eq!(m.parent_id, None);
    }

    #[test]
    fn set_all() {
        let m = parse_migration(r###"
            CREATE MIGRATION {
                    set id := 'u234';
                    set parent_id := 'u123';
                    set message := $$ hello world! $$;
            };
        "###)
            .unwrap();
        assert_eq!(m.id, Some("u234".into()));
        assert_eq!(m.message, Some(" hello world! ".into()));
        assert_eq!(m.parent_id, Some("u123".into()));
    }

    #[test]
    fn set_duplicate() {
        let m = parse_migration(r###"
            CREATE MIGRATION {
                    set id := 'u234';
                    set id := 'u123';
            };
        "###)
            .unwrap_err();
        assert_eq!(m.to_string(),
            "parse error: Parse error at 2:13\n\
             duplicate `SET id` statement\n");
    }

    #[test]
    fn mix() {
        let m = parse_migration(r###"
            CREATE MIGRATION {
                    select 1;
                    set id := 'u234';
                    set some_thing := 123;
                    insert x;
                    set other_thing := 'test';
                    set thing3 := call(235);
            };
        "###)
            .unwrap();
        assert_eq!(m.id, Some("u234".into()));
        assert_eq!(m.message, None);
        assert_eq!(m.parent_id, None);
    }

    #[test]
    fn mix_braces() {
        let m = parse_migration(r###"
            CREATE MIGRATION {
                    SELECT Obj1 { field1 };
                    set id := 'u234';
            };
        "###)
            .unwrap();
        assert_eq!(m.id, Some("u234".into()));
        assert_eq!(m.message, None);
        assert_eq!(m.parent_id, None);
    }

    #[test]
    fn err_set1() {
        parse_migration(r###"
            CREATE MIGRATION {
                set id := 234;
            };
        "###).unwrap_err();
    }

    #[test]
    fn err_set2() {
        parse_migration(r###"
            CREATE MIGRATION {
                set id := 'u123' test;
            };
        "###).unwrap_err();
    }

    #[test]
    fn err_set3() {
        parse_migration(r###"
            CREATE MIGRATION {
                set id := 'u123' test;
            };
            something
        "###).unwrap_err();
    }
}

