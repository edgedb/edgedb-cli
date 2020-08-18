use std::fs;
use std::default::Default;
use std::str;

use codespan::Files;
use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle};
use codespan_reporting::term::{emit};
use termcolor::{StandardStream, ColorChoice};

use edgedb_protocol::error_response::ErrorResponse;
use edgedb_protocol::error_response::FIELD_POSITION_START;
use edgedb_protocol::error_response::FIELD_POSITION_END;
use edgedb_protocol::error_response::{FIELD_HINT, FIELD_DETAILS};
use edgedb_protocol::error_response::FIELD_SERVER_TRACEBACK;

use crate::migrations::source_map::SourceMap;
use crate::migrations::create::SourceName;


pub fn print_migration_error(err: &ErrorResponse,
    source_map: &SourceMap<SourceName>)
    -> Result<(), anyhow::Error>
{
    let pstart = err.attributes.get(&FIELD_POSITION_START)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok());
    let pend = err.attributes.get(&FIELD_POSITION_END)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok());
    let (pstart, pend) = match (pstart, pend) {
        (Some(s), Some(e)) => (s as usize, e as usize),
        _ => {
            eprintln!("{}", err.display(false));
            return Ok(());
        }
    };
    let (file_name, pstart, pend) =
        match source_map.translate_range(pstart, pend) {
            Ok((SourceName::File(path), offset)) => {
                (path, pstart - offset, pend - offset)
            }
            _ => {
                eprintln!("{}", err.display(false));
                return Ok(());
            }
        };
    let data = match fs::read_to_string(file_name) {
        Ok(data) => data,
        Err(_) => {
            eprintln!("{}", err.display(false));
            return Ok(());
        }
    };

    let hint = err.attributes.get(&FIELD_HINT)
        .and_then(|x| str::from_utf8(x).ok())
        .unwrap_or("error");
    let detail = err.attributes.get(&FIELD_DETAILS)
        .and_then(|x| String::from_utf8(x.to_vec()).ok());
    let mut files = Files::new();
    let file_id = files.add(file_name, data);
    let diag = Diagnostic::error()
        .with_message(&err.message)
        .with_labels(vec![
            Label {
                file_id,
                style: LabelStyle::Primary,
                range: pstart as usize..pend as usize+1,
                message: hint.into(),
            },
        ])
        .with_notes(detail.into_iter().collect());

    emit(&mut StandardStream::stderr(ColorChoice::Auto),
        &Default::default(), &files, &diag)?;

    if err.code == 0x_01_00_00_00 {
        let tb = err.attributes.get(&FIELD_SERVER_TRACEBACK);
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
