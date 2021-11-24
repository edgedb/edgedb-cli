use std::default::Default;
use std::str;

use codespan_reporting::files::SimpleFile;
use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle};
use codespan_reporting::term::{emit};
use termcolor::{StandardStream, ColorChoice};

use edgedb_client::errors::{Error, InternalServerError};

use crate::print;


pub fn print_query_error(err: &Error, query: &str, verbose: bool)
    -> Result<(), anyhow::Error>
{
    let pstart = err.position_start();
    let pend = err.position_end();
    let (pstart, pend) = match (pstart, pend) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            print::edgedb_error(&err, verbose);
            return Ok(());
        }
    };
    let hint = err.hint().unwrap_or("error");
    let detail = err.details().map(|s| s.into());
    let files = SimpleFile::new("query", query);
    let context_error = err
        .contexts()
        .rev()
        .collect::<Vec<_>>();
    if context_error.len() > 0 {
        print::error(context_error.join(": "));
    }
    let diag = Diagnostic::error()
        .with_message(&format!(
            "{}{}",
            err.kind_name(),
            err
                .initial_message()
                .map(|s| format!(": {}", s))
                .unwrap_or("".into())
        ))
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

    if err.is::<InternalServerError>() || verbose {
        if let Some(traceback) = err.server_traceback() {
            eprintln!("  Server traceback:");
            for line in traceback.lines() {
                eprintln!("      {}", line);
            }
        }
    }
    Ok(())
}
