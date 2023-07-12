use edgeql_parser::keywords::Keyword;
use edgeql_parser::tokenizer::{Kind, Token, Tokenizer};

pub fn is_analyze(query: &str) -> bool {
    match (&mut Tokenizer::new(query)).next() {
        Some(Ok(Token {
            kind: Kind::Keyword(Keyword("analyze")),
            ..
        })) => true,
        Some(Ok(_) | Err(_)) => false, // let EdgeDB handle Err
        None => false,                 // but should be unreachable
    }
}
