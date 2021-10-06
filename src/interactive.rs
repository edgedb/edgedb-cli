use std::collections::HashMap;
use std::mem::replace;
use std::str;
use std::time::Instant;

use anyhow::{self, Context};
use async_std::task;
use async_std::prelude::{StreamExt, FutureExt};
use async_std::io::stdout;
use async_std::io::prelude::WriteExt;
use async_std::channel::{bounded as channel};
use bytes::{Bytes, BytesMut};
use colorful::Colorful;

use edgedb_client::errors::{ErrorKind, ClientEncodingError};
use edgedb_protocol::client_message::ClientMessage;
use edgedb_protocol::client_message::{DescribeStatement, DescribeAspect};
use edgedb_protocol::client_message::{Execute};
use edgedb_protocol::client_message::{Prepare, IoFormat, Cardinality};
use edgedb_protocol::query_arg::{Encoder, QueryArgs};
use edgedb_protocol::server_message::ServerMessage;
use edgedb_protocol::value::Value;
use edgeql_parser::preparser::{self, full_statement};

use crate::commands::{backslash, ExitCode};
use crate::config::Config;
use crate::echo;
use crate::error_display::print_query_error;
use crate::interrupt::{Interrupt, InterruptError};
use crate::options::Options;
use crate::outputs::tab_separated;
use crate::print::Highlight;
use crate::print::{self, PrintError};
use crate::prompt;
use crate::repl;
use crate::variables::input_variables;


const QUERY_OPT_IMPLICIT_LIMIT: u16 = 0xFF01;
const QUERY_OPT_INLINE_TYPENAMES: u16 = 0xFF02;

#[derive(Debug, thiserror::Error)]
#[error("Shutting down on user request")]
pub struct CleanShutdown;

#[derive(Debug, thiserror::Error)]
#[error("QueryError")]
pub struct QueryError;

struct ToDo<'a> {
    tail: &'a str,
}

#[derive(Debug, PartialEq)]
pub enum ToDoItem<'a> {
    Query(&'a str),
    Backslash(&'a str),
}

impl ToDo<'_> {
    fn new(source: &str) -> ToDo {
        ToDo { tail: source.trim() }
    }
}

impl<'a> Iterator for ToDo<'a> {
    type Item = ToDoItem<'a>;
    fn next(&mut self) -> Option<ToDoItem<'a>> {
        loop {
            let tail = self.tail.trim_start();
            if tail.starts_with("\\") {
                let len = backslash::full_statement(&tail);
                self.tail = &tail[len..];
                return Some(ToDoItem::Backslash(&tail[..len]));
            } else if preparser::is_empty(tail) {
                return None;
            } else {
                let len = full_statement(&tail.as_bytes(), None)
                    .unwrap_or(tail.len());
                self.tail = &tail[len..];
                if preparser::is_empty(&tail[..len]) {
                    continue;
                } else {
                    return Some(ToDoItem::Query(&tail[..len]));
                }
            }
        }
    }
}


pub fn main(options: Options, cfg: Config) -> Result<(), anyhow::Error> {
    let (control_wr, control_rd) = channel(1);
    let (repl_wr, repl_rd) = channel(1);
    let conn = options.create_connector()?;
    let limit = cfg.shell.limit.unwrap_or(100);
    let print = print::Config::new()
        .max_items(limit)
        .expand_strings(cfg.shell.expand_strings.unwrap_or(true))
        .implicit_properties(cfg.shell.implicit_properties.unwrap_or(false))
        .colors(atty::is(atty::Stream::Stdout))
        .clone();
    let state = repl::State {
        prompt: repl::PromptRpc {
            control: control_wr,
            data: repl_rd,
        },
        print,
        verbose_errors: cfg.shell.verbose_errors.unwrap_or(false),
        last_error: None,
        implicit_limit: Some(limit),
        output_format: options.output_format
            .or(cfg.shell.output_format)
            .unwrap_or(repl::OutputFormat::Default),
        display_typenames: cfg.shell.display_typenames.unwrap_or(true),
        input_mode: cfg.shell.input_mode.unwrap_or(repl::InputMode::Emacs),
        print_stats: cfg.shell.print_stats.unwrap_or(repl::PrintStats::Off),
        history_limit: cfg.shell.history_size.unwrap_or(10000),
        database: conn.get()?.get_database().into(),
        conn_params: conn,
        last_version: None,
        connection: None,
        initial_text: "".into(),
    };
    let handle = task::spawn(_main(options, state, cfg));
    prompt::main(repl_wr, control_rd)?;
    task::block_on(handle)?;
    Ok(())
}

pub async fn _main(options: Options, mut state: repl::State, cfg: Config)
    -> anyhow::Result<()>
{
    let mut conn = state.conn_params.connect().await?;
    let fetched_version = conn.get_version().await?;
    echo!("EdgeDB".light_gray(), fetched_version[..].light_gray(),
        format_args!("(repl {})", env!("CARGO_PKG_VERSION")).fade());
    state.last_version = Some(fetched_version);
    if let Some(config_path) = &cfg.file_name {
        echo!(
            format_args!("Applied {} configuration file",
                config_path.display(),
            ).fade());
    }
    echo!(r#"Type \help for help, \quit to quit."#.light_gray());
    state.set_history_limit(state.history_limit).await?;
    state.connection = Some(conn);
    match _interactive_main(&options, &mut state).await {
        Ok(()) => return Ok(()),
        Err(e) => {
            if e.is::<CleanShutdown>() {
                return Ok(());
            }
            return Err(e);
        }
    }
}

fn _check_json_limit(json: &serde_json::Value, path: &mut String, limit: usize)
    -> bool
{
    use serde_json::Value::*;
    use std::fmt::Write;

    let level = path.len();
    match json {
        Array(items) => {
            if items.len() > limit {
                return false;
            }
            for (idx, item) in items.iter().enumerate() {
                write!(path, "[{}]", idx).expect("formatting failed");
                _check_json_limit(item, path, limit);
                path.truncate(level);
            }
        }
        Object(pairs) => {
            for (key, value) in pairs {
                write!(path, ".{}", key).expect("formatting failed");
                _check_json_limit(value, path, limit);
                path.truncate(level);
            }
        }
        _ => {}
    }
    return true;
}

fn print_json_limit_error(path: &str) {
    eprintln!("Error: Cannot render JSON result: {} is too long. \
        Consider adding an explicit LIMIT clause, \
        or increasing the implicit limit using `\\set limit`.",
        if path.is_empty() { "." } else { path });
}

fn check_json_limit(json: &serde_json::Value, path: &str, limit: usize) -> bool
{
    let mut path_buf = path.to_owned();
    if !_check_json_limit(json, &mut path_buf, limit) {
        print_json_limit_error(&path_buf);
        return false;
    }
    return true;
}

async fn execute_backslash(mut state: &mut repl::State, text: &str)
    -> anyhow::Result<()>
{
    use backslash::ExecuteResult::*;

    let cmd = match backslash::parse(text) {
        Ok(cmd) => cmd,
        Err(e) => {
            if e.help {
                println!("{}", e.message);
            } else {
                eprintln!("Error parsing backslash command: {}",
                          e.message);
            }
            // Quick-edit command on error
            state.initial_text = text.into();
            return Ok(());
        }
    };
    let res = backslash::execute(&cmd.command, &mut state).await;
    match res {
        Ok(Skip) => {},
        Ok(Quit) => {
            state.terminate().await;
            return Err(CleanShutdown)?;
        }
        Ok(Input(text)) => state.initial_text = text,
        Err(e) => {
            if e.is::<ExitCode>() {
                // It's expected that command already printed all required
                // messages, so ignoring it is safe
            } else {
                eprintln!("Error executing command: {:#}", e);
                // Quick-edit command on error
                state.initial_text = text.into();
                state.last_error = Some(e);
            }
        }
    }
    Ok(())
}

async fn execute_query(options: &Options, mut state: &mut repl::State,
    statement: &str)
    -> anyhow::Result<()>
{
    use crate::repl::OutputFormat::*;
    use crate::repl::PrintStats::*;
    let start = Instant::now();

    let statement_name = Bytes::from_static(b"");

    let mut headers = HashMap::new();
    if let Some(implicit_limit) = state.implicit_limit {
        headers.insert(
            QUERY_OPT_IMPLICIT_LIMIT,
            Bytes::from(format!("{}", implicit_limit+1)));
    }
    let cli = state.connection.as_mut().expect("connection established");

    if state.display_typenames && cli.protocol().supports_inline_typenames() {
        headers.insert(QUERY_OPT_INLINE_TYPENAMES,
                       Bytes::from_static(b"true"));
    }

    let start_prepare = Instant::now();
    let mut seq = cli.start_sequence().await?;
    seq.send_messages(&[
        ClientMessage::Prepare(Prepare {
            headers,
            io_format: match state.output_format {
                Default | TabSeparated => IoFormat::Binary,
                JsonLines | JsonPretty => IoFormat::JsonElements,
                Json => IoFormat::Json,
            },
            expected_cardinality: Cardinality::Many,
            statement_name: statement_name.clone(),
            command_text: String::from(statement),
        }),
        ClientMessage::Flush,
    ]).await?;

    loop {
        let msg = seq.message().await?;
        match msg {
            ServerMessage::PrepareComplete(..) => {
                break;
            }
            ServerMessage::ErrorResponse(err) => {
                let err = err.into();
                print_query_error(&err, statement, state.verbose_errors)?;
                state.last_error = Some(err.into());
                seq.err_sync().await?;
                return Err(QueryError)?;
            }
            _ => {
                eprintln!("WARNING: unsolicited message {:?}", msg);
            }
        }
    }
    if state.print_stats == Detailed {
        eprintln!("{}",
            format!("Prepare: {:?}", start_prepare.elapsed()).dark_gray());
    }

    let start_describe = Instant::now();
    seq.send_messages(&[
        ClientMessage::DescribeStatement(DescribeStatement {
            headers: HashMap::new(),
            aspect: DescribeAspect::DataDescription,
            statement_name: statement_name.clone(),
        }),
        ClientMessage::Flush,
    ]).await?;

    let data_description = loop {
        let msg = seq.message().await?;
        match msg {
            ServerMessage::CommandDataDescription(data_desc) => {
                break data_desc;
            }
            ServerMessage::ErrorResponse(err) => {
                let err = err.into();
                print_query_error(&err, statement, state.verbose_errors)?;
                state.last_error = Some(err.into());
                seq.err_sync().await?;
                return Err(QueryError)?;
            }
            _ => {
                eprintln!("WARNING: unsolicited message {:?}", msg);
            }
        }
    };
    if state.print_stats == Detailed {
        eprintln!("{}",
            format!("Describe: {:?}", start_describe.elapsed()).dark_gray());
    }
    if options.debug_print_descriptors {
        println!("Descriptor: {:?}", data_description);
    }
    let desc = data_description.output()?;
    let indesc = data_description.input()?;
    if options.debug_print_descriptors {
        println!("Input Descr {:#?}", indesc.descriptors());
        println!("Output Descr {:#?}", desc.descriptors());
    }
    let codec = desc.build_codec()?;
    if options.debug_print_codecs {
        println!("Codec {:#?}", codec);
    }
    if options.debug_print_codecs {
        let incodec = indesc.build_codec()?;
        println!("Input Codec {:#?}", incodec);
    }

    let first_part = start.elapsed();
    let input = match input_variables(&indesc, &mut state.prompt).await {
        Ok(input) => input,
        Err(e) => {
            eprintln!("{:#}", e);
            state.last_error = Some(e);
            seq.end_clean();
            return Err(QueryError)?;
        }
    };

    let start_execute = Instant::now();
    let mut arguments = BytesMut::with_capacity(8);
    input.encode(&mut Encoder::new(
        &indesc.as_query_arg_context(),
        &mut arguments,
    )).map_err(ClientEncodingError::with_source)?;

    seq.send_messages(&[
        ClientMessage::Execute(Execute {
            headers: HashMap::new(),
            statement_name: statement_name.clone(),
            arguments: arguments.freeze(),
        }),
        ClientMessage::Sync,
    ]).await?;

    let mut items = seq.response(codec);
    if desc.root_pos().is_none() {
        match items.get_completion().await {
            Ok(ref val) => print::completion(val),
            Err(e) => {
                eprintln!("Error: {}", e);
                state.last_error = Some(e.into());
                return Err(QueryError)?;
            }
        }
        return Ok(());
    }

    let mut cfg = state.print.clone();
    if let Some((w, _h)) = term_size::dimensions_stdout() {
        // update max_width each time
        cfg.max_width(w);
    }
    match state.output_format {
        TabSeparated => {
            let mut index = 0;
            while let Some(row) = items.next().await.transpose()? {
                if index == 0 && state.print_stats == Detailed {
                    eprintln!("{}",
                        format!("First row: {:?}", start_execute.elapsed())
                        .dark_gray()
                    );
                }
                if let Some(limit) = state.implicit_limit {
                    if index >= limit {
                        eprintln!("Error: Too many rows. Consider \
                            adding an explicit LIMIT clause, \
                            or increasing the implicit limit \
                            using `\\set limit`.");
                        items.skip_remaining().await?;
                        return Err(QueryError)?;
                    }
                }
                let mut text = match tab_separated::format_row(&row) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        // exhaust the iterator to get connection in the
                        // consistent state
                        items.skip_remaining().await?;
                        return Err(QueryError)?;
                    }
                };
                // trying to make writes atomic if possible
                text += "\n";
                stdout().write_all(text.as_bytes()).await?;
                index += 1;
            }
        }
        Default => {
            match print::native_to_stdout(items, &cfg).await {
                Ok(()) => {}
                Err(e) => {
                    match e {
                        PrintError::StreamErr {
                            source: ref error,
                            ..
                        } => {
                            eprintln!("{:#}", error);
                        }
                        _ => eprintln!("{:#?}", e),
                    }
                    state.last_error = Some(e.into());
                    return Err(QueryError)?;
                }
            }
            println!();
        }
        Json => {
            let mut index = 0;
            while let Some(row) = items.next().await.transpose()? {
                if index == 0 && state.print_stats == Detailed {
                    eprintln!("{}",
                        format!("First row: {:?}", start_execute.elapsed())
                        .dark_gray()
                    );
                }
                index += 1;
                let text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "the server returned a non-string value in JSON mode")),
                };
                let jitems: serde_json::Value;
                jitems = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                if let Some(limit) = state.implicit_limit {
                    if !check_json_limit(&jitems, "", limit) {
                        items.skip_remaining().await?;
                        return Err(QueryError)?;
                    }
                }
                let jitems = jitems.as_array()
                    .ok_or_else(|| anyhow::anyhow!(
                        "the server returned a non-array value \
                         in JSON mode"))?;
                // trying to make writes atomic if possible
                let mut data = print::json_to_string(jitems, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
            }
        }
        JsonPretty | JsonLines => {
            let mut index = 0;
            while let Some(row) = items.next().await.transpose()? {
                if index == 0 && state.print_stats == Detailed {
                    eprintln!("{}",
                        format!("First row: {:?}", start_execute.elapsed())
                        .dark_gray()
                    );
                }
                let mut text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "the server returned a non-string value in JSON mode")),
                };
                let value: serde_json::Value;
                value = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                let path = format!(".[{}]", index);
                if let Some(limit) = state.implicit_limit {
                    if index >= limit {
                        print_json_limit_error(&path);
                        items.skip_remaining().await?;
                        return Err(QueryError)?;
                    }
                    if !check_json_limit(&value, &path, limit) {
                        items.skip_remaining().await?;
                        return Err(QueryError)?;
                    }
                }
                if state.output_format == JsonLines {
                    // trying to make writes atomic if possible
                    text += "\n";
                    stdout().write_all(text.as_bytes()).await?;
                } else {
                    // trying to make writes atomic if possible
                    let mut data;
                    data = print::json_item_to_string(&value, &cfg)?;
                    data += "\n";
                    stdout().write_all(data.as_bytes()).await?;
                    index += 1;
                }
            }
        }
    }
    if state.print_stats != Off {
        eprintln!("{}",
            format!("Query time (including output formatting): {:?}",
            first_part + start_execute.elapsed())
            .dark_gray()
        );
    }
    state.last_error = None;
    return Ok(());
}

async fn _interactive_main(options: &Options, state: &mut repl::State)
    -> Result<(), anyhow::Error>
{
    let ctrlc = Interrupt::ctrl_c();
    loop {
        state.ensure_connection()
            .race(ctrlc.wait_result())
            .await?;
        let cur_initial = replace(&mut state.initial_text, String::new());
        let inp = match state.edgeql_input(&cur_initial).await? {
            prompt::Input::Eof => {
                state.terminate()
                    .race(async { ctrlc.wait().await; })
                    .await;
                return Err(CleanShutdown)?;
            }
            prompt::Input::Interrupt => {
                continue;
            }
            prompt::Input::Text(inp) => inp,
            prompt::Input::Value(_) => unreachable!(),
        };
        for item in ToDo::new(&inp) {
            let result = match item {
                ToDoItem::Backslash(text) => {
                    execute_backslash(state, text)
                        .race(ctrlc.wait_result())
                        .await
                }
                ToDoItem::Query(statement) => {
                    state.soft_reconnect()
                        .race(ctrlc.wait_result())
                        .await?;
                    execute_query(options, state, statement)
                        .race(ctrlc.wait_result())
                        .await
                }
            };
            if let Err(err) = result {
                if err.is::<InterruptError>() {
                    eprintln!("Interrupted.");
                    state.reconnect()
                        .race(ctrlc.wait_result())
                        .await?;
                } else if err.is::<CleanShutdown>() {
                    return Err(err)?;
                } else if !err.is::<QueryError>() {
                    print::error(err);
                }
                // Don't continue next statements on error
                break;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::{ToDo, ToDoItem};

    #[test]
    fn double_semicolon() {
        assert_eq!(
            ToDo::new("SELECT 1;;SELECT 2").collect::<Vec<_>>(),
            &[
                ToDoItem::Query("SELECT 1;"),
                ToDoItem::Query("SELECT 2"),
            ]);
    }
}
