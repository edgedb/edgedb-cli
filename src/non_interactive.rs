use std::str;
use std::io::{stdout, Write};

use anyhow::{self, Context};
use bytes::{BytesMut};
use is_terminal::IsTerminal;
use terminal_size::{Width, terminal_size};
use tokio::fs::{File as AsyncFile};
use tokio::io::{AsyncRead, stdin};

use edgedb_protocol::client_message::{CompilationOptions};
use edgedb_protocol::client_message::{IoFormat, Cardinality};
use edgedb_protocol::common::{Capabilities};
use edgedb_protocol::value::Value;
use edgeql_parser::preparser;
use tokio_stream::StreamExt;

use crate::classify;
use crate::commands::ExitCode;
use crate::connect::Connection;
use crate::error_display::print_query_error;
use crate::options::Options;
use crate::options::Query;
use crate::outputs::tab_separated;
use crate::print::{self, PrintError};
use crate::repl::OutputFormat;
use crate::statement::{read_statement, EndOfFile};


#[tokio::main(flavor = "current_thread")]
pub async fn noninteractive_main(q: &Query, options: &Options)
    -> Result<(), anyhow::Error>
{
    // There's some extra complexity here due to the fact that we
    // have to support now deprecated top-level `--json` and
    // `--tab-separated` flags.
    let fmt = if let Some(of) = q.output_format {
        // If the new `--output-format` option was provided - use it.
        of
    } else {
        // Or else, check what's coming from the `main::main()`
        // entrypoint.
        if let Some(fmt) = options.output_format {
            fmt
        } else {
            // Means "native" serialization; for `edgedb query`
            // the default is `json-pretty`.
            OutputFormat::JsonPretty
        }
    };

    if let Some(filename) = &q.file {
        if filename == "-" {
            interpret_file(&mut stdin(), options, fmt).await?;
        } else {
            let mut file = AsyncFile::open(filename).await?;
            interpret_file(&mut file, options, fmt).await?;
        }
    } else if let Some(queries) = &q.queries {
        let mut conn = options.create_connector().await?.connect().await?;
        for query in queries {
            if classify::is_analyze(query) {
                anyhow::bail!("Analyze queries are not allowed. \
                               Use the dedicated `edgedb analyze` command.");
            }
            run_query(&mut conn, query, &options, fmt).await?;
        }
    } else {
        print::error("either a --file option or \
                     a <queries> positional argument is required.");
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
pub async fn interpret_stdin(options: &Options, fmt: OutputFormat)
    -> Result<(), anyhow::Error>
{
    return interpret_file(&mut stdin(), options, fmt).await;
}

async fn interpret_file<T>(file: &mut T, options: &Options, fmt: OutputFormat)
    -> Result<(), anyhow::Error>
    where T: AsyncRead + Unpin
{
    let mut conn = options.create_connector().await?.connect().await?;
    let mut inbuf = BytesMut::with_capacity(8192);
    loop {
        let stmt = match read_statement(&mut inbuf, file).await {
            Ok(chunk) => chunk,
            Err(e) if e.is::<EndOfFile>() => break,
            Err(e) => return Err(e),
        };
        let stmt = str::from_utf8(&stmt[..])
            .context("can't decode statement")?;
        if preparser::is_empty(stmt) {
            continue;
        }
        if classify::is_analyze(stmt) {
            anyhow::bail!("Analyze queries are not allowed. \
                           Use the dedicated `edgedb analyze` command.");
        }
        run_query(&mut conn, &stmt, &options, fmt).await?;
    }
    Ok(())
}

async fn run_query(conn: &mut Connection, stmt: &str, options: &Options,
    fmt: OutputFormat)
    -> Result<(), anyhow::Error>
{
    _run_query(conn, stmt, options, fmt).await.map_err(|err| {
        if let Some(err) = err.downcast_ref::<edgedb_errors::Error>() {
            match print_query_error(&err, stmt, false, "<query>") {
                Ok(()) => ExitCode::new(1).into(),
                Err(e) => e,
            }
        } else {
            err
        }
    })
}

async fn _run_query(conn: &mut Connection, stmt: &str, _options: &Options,
    fmt: OutputFormat)
    -> Result<(), anyhow::Error>
{
    use crate::repl::OutputFormat::*;

    let flags = CompilationOptions {
        implicit_limit: None,
        implicit_typenames: fmt == Default &&
            conn.protocol().supports_inline_typenames(),
        implicit_typeids: false,
        explicit_objectids: true,
        allow_capabilities: Capabilities::ALL,
        io_format: match fmt {
            Default | TabSeparated => IoFormat::Binary,
            JsonLines | JsonPretty => IoFormat::JsonElements,
            Json => IoFormat::Json,
        },
        expected_cardinality: Cardinality::Many,
    };
    let data_description = conn.parse(&flags, stmt).await?;

    let mut cfg = print::Config::new();
    if let Some((Width(w), _h)) = terminal_size() {
        cfg.max_width(w.into());
    }
    cfg.colors(stdout().is_terminal());

    let mut items = conn.execute_stream(
        &flags, stmt, &data_description, &(),
    ).await?;

    if !items.can_contain_data() {
        let res = items.complete().await?;
        print::completion(&res.status_data);
        return Ok(());
    }

    match fmt {
        OutputFormat::TabSeparated => {
            while let Some(row) = items.next().await.transpose()? {
                let mut text = tab_separated::format_row(&row)?;
                // trying to make writes atomic if possible
                text += "\n";
                stdout().lock().write_all(text.as_bytes())?;
            }
        }
        OutputFormat::Default => {
            match print::native_to_stdout(items, &cfg).await {
                Ok(()) => {}
                Err(e) => {
                    match e {
                        PrintError::StreamErr { source: ref error, ..  } => {
                            print::error(error);
                        }
                        _ => {
                            print::error(e);
                        },
                    }
                    return Ok(());
                }
            }
        }
        OutputFormat::JsonPretty => {
            while let Some(row) = items.next().await.transpose()? {
                let text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "the server returned \
                         a non-string value in JSON mode")),
                };
                let value: serde_json::Value = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                // trying to make writes atomic if possible
                let mut data = print::json_item_to_string(&value, &cfg)?;
                data += "\n";
                stdout().lock().write_all(data.as_bytes())?;
            }
        }
        OutputFormat::JsonLines => {
            while let Some(row) = items.next().await.transpose()? {
                let mut text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "the server returned \
                         a non-string value in JSON mode")),
                };
                // trying to make writes atomic if possible
                text += "\n";
                stdout().lock().write_all(text.as_bytes())?;
            }
        }
        OutputFormat::Json => {
            while let Some(row) = items.next().await.transpose()? {
                let text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "the server returned \
                         a non-string value in JSON mode")),
                };
                let items: serde_json::Value = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                let items = items.as_array()
                    .ok_or_else(|| anyhow::anyhow!(
                        "the server returned a non-array value in JSON mode"))?;
                // trying to make writes atomic if possible
                let mut data = print::json_to_string(items, &cfg)?;
                data += "\n";
                stdout().lock().write_all(data.as_bytes())?;
            }
        }
    }
    Ok(())
}
