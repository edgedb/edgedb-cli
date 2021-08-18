use std::default::Default;
use std::fs;
use std::path::Path;
use std::str;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle};
use codespan_reporting::term::{emit};
use termcolor::{StandardStream, ColorChoice};

use edgedb_client::errors::{Error, InternalServerError};
use edgedb_protocol::error_response::FIELD_POSITION_END;
use edgedb_protocol::error_response::FIELD_POSITION_START;
use edgedb_protocol::error_response::{FIELD_HINT, FIELD_DETAILS};
use edgedb_protocol::error_response::display_error;
use edgeql_parser::tokenizer::TokenStream;
use edgedb_protocol::error_response::FIELD_SERVER_TRACEBACK;

use crate::migrations::source_map::SourceMap;
use crate::migrations::create::SourceName;


fn end_of_last_token(data: &str) -> Option<u64> {
    let mut tokenizer = TokenStream::new(data);
    let mut off = 0;
    for tok in &mut tokenizer {
        off = tok.ok()?.end.offset;
    }
    return Some(off);
}

fn get_error_info<'x>(err: &Error, source_map: &'x SourceMap<SourceName>)
    -> Option<(&'x Path, String, usize, usize, bool)>
{
    let pstart = err.headers().get(&FIELD_POSITION_START)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok())? as usize;
    let pend = err.headers().get(&FIELD_POSITION_END)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok())? as usize;
    let (src, offset) = source_map.translate_range(pstart, pend).ok()?;
    let res = match src {
        SourceName::File(path) => {
            let data = fs::read_to_string(&path).ok()?;
            (path.as_ref(), data, pstart - offset, pend - offset, false)
        }
        SourceName::Semicolon(path) => {
            let data = fs::read_to_string(&path).ok()?;
            let tok_offset = end_of_last_token(&data)? as usize;
            (path.as_ref(), data, tok_offset, tok_offset, true)
        }
        _ => return None,
    };
    return Some(res);
}

pub fn print_migration_error(err: &Error, source_map: &SourceMap<SourceName>)
    -> Result<(), anyhow::Error>
{
    let (file_name, data, pstart, pend, eof) =
        match get_error_info(err, source_map) {
            Some(pair) => pair,
            None => {
                eprintln!("{}", display_error(err, false));
                return Ok(());
            }
        };

    let message = if eof {
        "Unexpected end of file"
    } else {
        &err.initial_message().unwrap_or(err.kind_name())
    };
    let hint = err.headers().get(&FIELD_HINT)
        .and_then(|x| str::from_utf8(x).ok())
        .unwrap_or("error");
    let detail = err.headers().get(&FIELD_DETAILS)
        .and_then(|x| String::from_utf8(x.to_vec()).ok());
    let file_name_display = file_name.display();
    let files = SimpleFile::new(&file_name_display, data);
    let diag = Diagnostic::error()
        .with_message(message)
        .with_labels(vec![
            Label {
                file_id: (),
                style: LabelStyle::Primary,
                range: pstart..pend,
                message: hint.into(),
            },
        ])
        .with_notes(detail.into_iter().collect());

    emit(&mut StandardStream::stderr(ColorChoice::Auto),
        &Default::default(), &files, &diag)?;

    if err.is::<InternalServerError>() {
        let tb = err.headers().get(&FIELD_SERVER_TRACEBACK);
        if let Some(traceback) = tb {
            if let Ok(traceback) = str::from_utf8(traceback) {
                eprintln!("  Server traceback:");
                for line in traceback.lines() {
                    eprintln!("      {}", line);
                }
            }
        }
    }
    Ok(())
}
