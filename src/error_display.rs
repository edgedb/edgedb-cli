use std::default::Default;
use std::str;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle};
use codespan_reporting::term::{emit};
use termcolor::{StandardStream, ColorChoice};

use edgedb_client::errors::{Error, InternalServerError};
use edgedb_protocol::error_response::FIELD_POSITION_START;
use edgedb_protocol::error_response::FIELD_POSITION_END;
use edgedb_protocol::error_response::{FIELD_HINT, FIELD_DETAILS};
use edgedb_protocol::error_response::FIELD_SERVER_TRACEBACK;
use edgedb_protocol::error_response::display_error;


pub fn print_query_error(err: &Error, query: &str, verbose: bool)
    -> Result<(), anyhow::Error>
{
    let pstart = err.headers().get(&FIELD_POSITION_START)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok());
    let pend = err.headers().get(&FIELD_POSITION_END)
       .and_then(|x| str::from_utf8(x).ok())
       .and_then(|x| x.parse::<u32>().ok());
    let (pstart, pend) = match (pstart, pend) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            eprintln!("{}", display_error(err, verbose));
            return Ok(());
        }
    };
    let hint = err.headers().get(&FIELD_HINT)
        .and_then(|x| str::from_utf8(x).ok())
        .unwrap_or("error");
    let detail = err.headers().get(&FIELD_DETAILS)
        .and_then(|x| String::from_utf8(x.to_vec()).ok());
    let files = SimpleFile::new("query", query);
    let diag = Diagnostic::error()
        .with_message(&format!("{:#}", err))
        .with_labels(vec![
            Label {
                file_id: (),
                style: LabelStyle::Primary,
                range: pstart as usize..pend as usize,
                message: hint.into(),
            },
        ])
        .with_notes(detail.into_iter().collect());

    emit(&mut StandardStream::stderr(ColorChoice::Auto),
        &Default::default(), &files, &diag)?;

    if err.is::<InternalServerError>() || verbose {
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
