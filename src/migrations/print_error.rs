use std::default::Default;
use std::fs;
use std::path::Path;
use std::str;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle};
use codespan_reporting::term::{emit};
use termcolor::{StandardStream, ColorChoice};

use edgedb_errors::{Error, InternalServerError};
use edgeql_parser::tokenizer::Tokenizer;

use crate::print;
use crate::migrations::source_map::SourceMap;
use crate::migrations::create::SourceName;


fn end_of_last_token(data: &str) -> Option<u64> {
    let mut tokenizer = Tokenizer::new(data);
    let mut off = 0;
    for tok in &mut tokenizer {
        off = tok.ok()?.span.end.offset;
    }
    return Some(off);
}

fn get_error_info<'x>(err: &Error, source_map: &'x SourceMap<SourceName>)
    -> Option<(&'x Path, String, usize, usize, bool)>
{
    let pstart = err.position_start()?;
    let pend = err.position_end()?;
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
                print::edgedb_error(err, false);
                return Ok(());
            }
        };

    let message = if eof {
        "Unexpected end of file"
    } else {
        &err.initial_message().unwrap_or(err.kind_name())
    };
    let hint = err.hint().unwrap_or("error");
    let detail = err.details().map(|s| s.into());
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
        if let Some(traceback) = err.server_traceback() {
            eprintln!("  Server traceback:");
            for line in traceback.lines() {
                eprintln!("      {}", line);
            }
        }
    }
    Ok(())
}
