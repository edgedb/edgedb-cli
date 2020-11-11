use std::str;

use anyhow::{self, Context};
use async_std::prelude::StreamExt;
use async_std::io::{stdin, stdout};
use async_std::io::prelude::WriteExt;

use bytes::BytesMut;
use edgeql_parser::preparser;
use edgedb_protocol::value::Value;

use crate::options::Options;
use crate::print::{self, PrintError};
use edgedb_client::reader::ReadError;
use crate::statement::{ReadStatement, EndOfFile};
use edgedb_client::client::Connection;
use edgedb_client::errors::NoResultExpected;
use crate::outputs::tab_separated;


pub async fn main(options: Options)
    -> Result<(), anyhow::Error>
{
    let mut conn = options.conn_params.connect().await?;
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
        query(&mut conn, &stmt, &options).await?;
    }
    Ok(())
}

pub async fn query(conn: &mut Connection, stmt: &str, options: &Options)
    -> Result<(), anyhow::Error>
{
    use crate::repl::OutputMode::*;
    let mut cfg = print::Config::new();
    if let Some((w, _h)) = term_size::dimensions_stdout() {
        cfg.max_width(w);
    }
    cfg.colors(atty::is(atty::Stream::Stdout));

    match options.output_mode {
        TabSeparated => {
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
        Default => {
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
                            eprintln!("{}", error);
                        }
                        _ => eprintln!("{:#?}", e),
                    }
                    return Ok(());
                }
            }
        }
        JsonElements => {
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
        Json => {
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
                        "non-array returned from postgres in JSON mode"))?;
                // trying to make writes atomic if possible
                let mut data = print::json_to_string(items, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
            }
        }
    }
    Ok(())
}
