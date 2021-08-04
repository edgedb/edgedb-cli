use std::str;

use anyhow::{self, Context};
use async_std::prelude::StreamExt;
use async_std::io::{stdin, stdout};
use async_std::io::prelude::WriteExt;
use async_std::fs::{read as async_read};

use bytes::BytesMut;
use edgeql_parser::preparser;
use edgedb_protocol::value::Value;

use crate::commands::ExitCode;
use crate::options::Options;
use crate::options::Query;
use crate::repl::OutputMode;
use crate::print::{self, PrintError};
use edgedb_client::reader::ReadError;
use crate::statement::{ReadStatement, EndOfFile};
use edgedb_client::client::Connection;
use edgedb_client::errors::NoResultExpected;
use crate::outputs::tab_separated;

use edgeql_parser::preparser::{full_statement};

pub async fn main(q: &Query, options: &Options)
    -> Result<(), anyhow::Error>
{
    let mut query_opts = options.clone();
    query_opts.output_mode = if q.tab_separated {
        OutputMode::TabSeparated
    } else if q.json {
        OutputMode::Json
    } else {
        // Let the top-level --json / --tab-seaparated propagate
        // (they are deprecated, though.)
        options.output_mode
    };

    if let Some(file) = &q.file {
        if file == "-" {
            interpret_stdin(query_opts).await?;
        } else {
            interpret_file(file.to_string(), query_opts).await?;
        }
    } else if let Some(queries) = &q.queries {
        let mut conn = options.create_connector()?.connect().await?;
        for query in queries {
            run_query(&mut conn, query, &query_opts).await?;
        }
    } else {
        eprintln!("error: either a --file option or \
                  a <queries> positional argument is required.");
    }

    Ok(())
}

pub async fn interpret_stdin(options: Options)
    -> Result<(), anyhow::Error>
{
    let mut conn = options.create_connector()?.connect().await?;
    let mut stdin = stdin();
    let mut inbuf = BytesMut::with_capacity(8192);
    loop {
        let stmt = match ReadStatement::new(&mut inbuf, &mut stdin).await {
            Ok(chunk) => chunk,
            Err(e) if e.is::<EndOfFile>() => break,
            Err(e) => return Err(e),
        };
        let stmt = str::from_utf8(&stmt[..])
            .context("can't decode statement")?;
        if preparser::is_empty(stmt) {
            continue;
        }
        run_query(&mut conn, &stmt, &options).await?;
    }
    Ok(())
}

async fn run_query(conn: &mut Connection, stmt: &str, options: &Options)
    -> Result<(), anyhow::Error>
{
    let mut cfg = print::Config::new();
    if let Some((w, _h)) = term_size::dimensions_stdout() {
        cfg.max_width(w);
    }
    cfg.colors(atty::is(atty::Stream::Stdout));

    match options.output_mode {
        OutputMode::TabSeparated => {
            let mut items = match
                conn.query_dynamic(stmt, &Value::empty_tuple()).await
            {
                Ok(items) => items,
                Err(e) => match e.downcast::<NoResultExpected>() {
                    Ok(e) => {
                        print::completion(&e.completion_message);
                        return Ok(());
                    }
                    Err(e) => Err(e)?,
                },
            };
            while let Some(row) = items.next().await.transpose()? {
                let mut text = tab_separated::format_row(&row)?;
                // trying to make writes atomic if possible
                text += "\n";
                stdout().write_all(text.as_bytes()).await?;
            }
        }
        OutputMode::Default => {
            let items = match
                conn.query_dynamic(stmt, &Value::empty_tuple()).await
            {
                Ok(items) => items,
                Err(e) => match e.downcast::<NoResultExpected>() {
                    Ok(e) => {
                        print::completion(&e.completion_message);
                        return Ok(());
                    }
                    Err(e) => Err(e)?,
                },
            };
            match print::native_to_stdout(items, &cfg).await {
                Ok(()) => {}
                Err(e) => {
                    match e {
                        PrintError::StreamErr {
                            source: ReadError::RequestError {
                                ref error, ..
                            },
                            ..
                        } => {
                            eprintln!("edgedb error: {}", error);
                        }
                        _ => eprintln!("edgedb error: {:#}", e),
                    }
                    return Ok(());
                }
            }
        }
        OutputMode::JsonElements => {
            let mut items = match
                conn.query_json_els(stmt, &Value::empty_tuple()).await
            {
                Ok(items) => items,
                Err(e) => match e.downcast::<NoResultExpected>() {
                    Ok(e) => {
                        print::completion(&e.completion_message);
                        return Ok(());
                    }
                    Err(e) => Err(e)?,
                },
            };
            while let Some(row) = items.next().await.transpose()? {
                let value: serde_json::Value = serde_json::from_str(&row)
                    .context("cannot decode json result")?;
                // trying to make writes atomic if possible
                let mut data = print::json_item_to_string(&value, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
            }
        }
        OutputMode::Json => {
            let mut items = match
                conn.query_json(stmt, &Value::empty_tuple()).await
            {
                Ok(items) => items,
                Err(e) => match e.downcast::<NoResultExpected>() {
                    Ok(e) => {
                        print::completion(&e.completion_message);
                        return Ok(());
                    }
                    Err(e) => Err(e)?,
                },
            };
            while let Some(row) = items.next().await.transpose()? {
                let items: serde_json::Value = serde_json::from_str(&row)
                    .context("cannot decode json result")?;
                let items = items.as_array()
                    .ok_or_else(|| anyhow::anyhow!(
                        "the server returned a non-array value in JSON mode"))?;
                // trying to make writes atomic if possible
                let mut data = print::json_to_string(items, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
            }
        }
    }
    Ok(())
}

fn truncated(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

async fn interpret_file(file: String, options: Options)
    -> Result<(), anyhow::Error>
{
    let mut contents = async_read(file).await?;
    contents.push(b';');

    let mut input = vec![];

    loop {
        match full_statement(&contents, None) {
            Ok(len) => {
                let stmt = String::from_utf8((&contents[..len]).to_vec())
                                .context("can't decode statement")?
                                .trim().to_string();
                if stmt != "" && stmt != ";" {
                    input.push(stmt);
                }

                contents = (&contents[len..]).to_vec();
                if contents.len() == 0 {
                    break
                }
            },
            Err(_) => {
                let remainder = String::from_utf8(contents)
                                .context("can't decode statement")?;
                if remainder.trim().len() != 0 {
                    eprintln!("The remainder of the file is not \
                              valid EdgeQL: {}", truncated(&remainder, 100));
                    return Err(ExitCode::new(1))?;
                }

                break
            }
        }
    }

    let mut conn = options.create_connector()?.connect().await?;
    for q in input {
        run_query(&mut conn, &q, &options).await?;
    }

    Ok(())
}
