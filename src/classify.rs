use edgeql_parser::tokenizer::{Tokenizer, Kind as TokenKind};

pub fn is_analyze(query: &str) -> bool {
    match (&mut Tokenizer::new(query)).next() {
        Some(Ok(tok))
        if tok.kind == TokenKind::Keyword &&
           tok.text.eq_ignore_ascii_case("analyze")
        => true,
        Some(Ok(_) | Err(_)) => false, // let EdgeDB handle Err
        None => false, // but should be unreachable
    }
}
