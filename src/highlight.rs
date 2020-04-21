use std::collections::HashSet;

use edgeql_parser::tokenizer::{TokenStream, Kind};
use edgeql_parser::keywords;
use crate::print::style::{Styler, Style};

lazy_static::lazy_static! {
    static ref UNRESERVED_KEYWORDS: HashSet<&'static str> =
        keywords::UNRESERVED_KEYWORDS.iter().map(|x| *x).collect();
}


pub fn edgeql(text: &str, styler: &Styler) -> String {
    let mut outbuf = String::with_capacity(text.len());
    let mut pos = 0;
    let mut token_stream = TokenStream::new(text);
    for res in &mut token_stream {
        let tok = match res {
            Ok(tok) => tok,
            Err(_) => {
                outbuf.push_str(&text[pos..]);
                return outbuf.into();
            }
        };
        if tok.start.offset as usize > pos {
            emit_insignificant(&mut outbuf, &styler,
                &text[pos..tok.start.offset as usize]);
        }
        if let Some(st) = token_style(tok.token.kind, tok.token.value)
        {
            styler.apply(st, tok.token.value, &mut outbuf);
        } else {
            outbuf.push_str(tok.token.value);
        }
        pos = tok.end.offset as usize;
    }
    emit_insignificant(&mut outbuf, &styler, &text[pos..]);
    return outbuf.into();
}

fn emit_insignificant(buf: &mut String, styler: &Styler, mut chunk: &str) {
    while let Some(pos) = chunk.find('#') {
        if let Some(end) = chunk[pos..].find('\n') {
            buf.push_str(&chunk[..pos]);
            styler.apply(Style::Comment, &chunk[pos..pos+end], buf);

            // must be unstyled to work well at the end of input
            buf.push('\n');

            chunk = &chunk[pos+end+1..];
        } else {
            break;
        }
    }
    buf.push_str(chunk);
}

fn token_style(kind: Kind, value: &str) -> Option<Style> {
    use edgeql_parser::tokenizer::Kind as T;
    use crate::print::style::Style as S;

    match kind {
        T::Keyword => Some(S::Keyword),

        T::At => Some(S::Punctuation),  // TODO(tailhook) but also decorators
        T::Dot => Some(S::Punctuation),
        T::ForwardLink => Some(S::Punctuation),
        T::BackwardLink => Some(S::Punctuation),

        T::Assign => None,
        T::SubAssign => None,
        T::AddAssign => None,
        T::Arrow => None,
        T::Coalesce => None,
        T::Namespace => None,
        T::FloorDiv => None,
        T::Concat => None,
        T::GreaterEq => None,
        T::LessEq => None,
        T::NotEq => None,
        T::NotDistinctFrom => None,
        T::DistinctFrom => None,
        T::Comma => None,
        T::OpenParen => None,
        T::CloseParen => None,
        T::OpenBracket => None,
        T::CloseBracket => None,
        T::OpenBrace => None,
        T::CloseBrace => None,
        T::Semicolon => None,
        T::Colon => None,
        T::Add => None,
        T::Sub => None,
        T::Mul => None,
        T::Div => None,
        T::Modulo => None,
        T::Pow => None,
        T::Less => None,
        T::Greater => None,
        T::Eq => None,
        T::Ampersand => None,
        T::Pipe => None,
        T::Argument => None, // TODO (tailhook)
        T::DecimalConst => Some(S::Constant),
        T::FloatConst => Some(S::Constant),
        T::IntConst => Some(S::Constant),
        T::BigIntConst => Some(S::Constant),
        T::BinStr => Some(S::String),
        T::Str => Some(S::String),
        T::BacktickName => None,
        T::Ident
        if UNRESERVED_KEYWORDS.contains(&value.to_lowercase()[..])
        => Some(S::Keyword),
        T::Ident => None,
    }
}
