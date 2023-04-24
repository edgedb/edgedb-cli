use edgeql_parser::tokenizer::{TokenStream, Kind as TokenKind};

pub fn is_analyze(query: &str) -> bool {
    match (&mut TokenStream::new(query)).next() {
        Some(Ok(tok))
        if tok.token.kind == TokenKind::Keyword &&
           tok.token.value.eq_ignore_ascii_case("analyze")
        => true,
        Some(Ok(_) | Err(_)) => false, // let EdgeDB handle Err
        None => false, // but should be unreachable
    }
}
