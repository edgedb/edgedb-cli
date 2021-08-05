use std::str;

use anyhow::{self, Context};
use async_std::prelude::StreamExt;
use async_std::io::{stdin, stdout, Read as AsyncRead};
use async_std::io::prelude::WriteExt;
use async_std::fs::{File as AsyncFile};

use bytes::BytesMut;
use edgeql_parser::preparser;
use edgedb_protocol::value::Value;

use crate::options::Options;
use crate::options::Query;
use crate::repl::OutputFormat;
use crate::print::{self, PrintError};
use edgedb_client::reader::ReadError;
use crate::statement::{ReadStatement, EndOfFile};
use edgedb_client::client::Connection;
use edgedb_client::errors::NoResultExpected;
use crate::outputs::tab_separated;

pub async fn main(q: &Query, options: &Options)
    -> Result<(), anyhow::Error>
{
    let mut query_opts = options.clone();

    // There's some extra complexity here due to the fact that we
    // have to support now deprecated top-level `--json` and
    // `--tab-separated` flags.
    query_opts.output_format = if let Some(of) = q.output_format {
        // If the new `--output-format` option was provided - use it.
        of
    } else {
        // Or else, check what's coming from the `main::main()`
        // entrypoint.
        if options.output_format == OutputFormat::Default {
            // Means "native" serialization; for `edgedb query`
            // the default is `json-pretty`.
            OutputFormat::JsonPretty
        } else {
            // If it's not Default, it must either be something set
            // with `--json` or `--tab-separated`, or it could be
            // the default `JsonLines` which is fine in this context.
            options.output_format
        }
    };

    if let Some(filename) = &q.file {
        if filename == "-" {
            interpret_stdin(query_opts).await?;
        } else {
            let mut file = AsyncFile::open(filename).await?;
            interpret_file(&mut file, query_opts).await?;
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
    return interpret_file(&mut stdin(), options).await;
}

async fn interpret_file<T>(file: &mut T, options: Options)
    -> Result<(), anyhow::Error>
    where T: AsyncRead + Unpin
{
    let mut conn = options.create_connector()?.connect().await?;
    let mut inbuf = BytesMut::with_capacity(8192);
    loop {
        let stmt = match ReadStatement::new(&mut inbuf, file).await {
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

    match options.output_format {
        OutputFormat::TabSeparated => {
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
        OutputFormat::Default => {
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
        OutputFormat::JsonPretty | OutputFormat::JsonLines => {
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
            if options.output_format == OutputFormat::JsonLines {
                while let Some(mut row) = items.next().await.transpose()? {
                    // trying to make writes atomic if possible
                    row += "\n";
                    stdout().write_all(row.as_bytes()).await?;
                }
            } else {
                while let Some(row) = items.next().await.transpose()? {
                    let value: serde_json::Value = serde_json::from_str(&row)
                        .context("cannot decode json result")?;
                    // trying to make writes atomic if possible
                    let mut data = print::json_item_to_string(&value, &cfg)?;
                    data += "\n";
                    stdout().write_all(data.as_bytes()).await?;
                }
            }
        }
        OutputFormat::Json => {
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
