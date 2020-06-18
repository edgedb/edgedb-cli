use std::collections::HashMap;
use std::mem::replace;
use std::str;

use anyhow::{self, Context};
use async_std::task;
use async_std::prelude::StreamExt;
use async_std::io::stdout;
use async_std::io::prelude::WriteExt;
use async_std::sync::{channel};
use bytes::{Bytes, BytesMut};
use colorful::Colorful;

use edgedb_protocol::client_message::ClientMessage;
use edgedb_protocol::client_message::{Prepare, IoFormat, Cardinality};
use edgedb_protocol::client_message::{DescribeStatement, DescribeAspect};
use edgedb_protocol::client_message::{Execute};
use edgedb_protocol::server_message::ServerMessage;
use edgedb_protocol::value::Value;
use edgeql_parser::preparser::full_statement;

use crate::commands::backslash;
use crate::options::Options;
use crate::print::{self, PrintError};
use crate::prompt;
use crate::reader::ReadError;
use crate::repl;
use crate::variables::input_variables;
use crate::error_display::print_query_error;
use crate::outputs::tab_separated;


const QUERY_OPT_IMPLICIT_LIMIT: u16 = 0xFF01;

#[derive(Debug, thiserror::Error)]
#[error("Shutting down on user request")]
pub struct CleanShutdown;

struct ToDo<'a> {
    tail: &'a str,
}

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
        let tail = self.tail.trim_start();
        if tail.starts_with("\\") {
            let len = backslash::full_statement(&tail);
            self.tail = &tail[len..];
            return Some(ToDoItem::Backslash(&tail[..len]));
        } else if tail.trim() == "" {
            return None;
        } else {
            let len = full_statement(&tail.as_bytes(), None)
                .unwrap_or(tail.len());
            self.tail = &tail[len..];
            return Some(ToDoItem::Query(&tail[..len]));
        }
    }
}


pub fn main(options: Options) -> Result<(), anyhow::Error> {
    let (control_wr, control_rd) = channel(1);
    let (repl_wr, repl_rd) = channel(1);
    let state = repl::State {
        prompt: repl::PromptRpc {
            control: control_wr,
            data: repl_rd,
        },
        print: print::Config::new()
            .max_items(100)
            .colors(atty::is(atty::Stream::Stdout))
            .clone(),
        verbose_errors: false,
        last_error: None,
        implicit_limit: Some(100),
        output_mode: options.output_mode,
        input_mode: repl::InputMode::Emacs,
        history_limit: 100,
        database: options.conn_params.get_effective_database(),
        conn_params: options.conn_params.clone(),
        last_version: None,
        connection: None,
        initial_text: "".into(),
    };
    let handle = task::spawn(_main(options, state));
    prompt::main(repl_wr, control_rd)?;
    task::block_on(handle)?;
    Ok(())
}

pub async fn _main(options: Options, mut state: repl::State)
    -> anyhow::Result<()>
{
    let mut conn = state.conn_params.connect().await?;
    let fetched_version = conn.get_version().await?;
    println!("{} {}",
        "EdgeDB".light_gray(),
        fetched_version[..].light_gray());
    state.last_version = Some(fetched_version);
    println!("{}", r#"Type "\?" for help."#.light_gray());
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
                write!(path, "[{}]", idx).expect("formatting succeeds");
                _check_json_limit(item, path, limit);
                path.truncate(level);
            }
        }
        Object(pairs) => {
            for (key, value) in pairs {
                write!(path, ".{}", key).expect("formatting succeeds");
                _check_json_limit(value, path, limit);
                path.truncate(level);
            }
        }
        _ => {}
    }
    return true;
}

fn print_json_limit_error(path: &str) {
    eprintln!("ERROR: Cannot render JSON result: {} is too long. \
        Consider putting an explicit LIMIT clause, \
        or increase the implicit limit using `\\set limit`.",
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
            eprintln!("Error parsing backslash command: {}",
                      e.message);
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
            return Ok(());
        }
        Ok(Input(text)) => state.initial_text = text,
        Err(e) => {
            eprintln!("Error executing command: {}", e);
            // Quick-edit command on error
            state.initial_text = text.into();
            state.last_error = Some(e);
        }
    }
    Ok(())
}

async fn execute_query(options: &Options, mut state: &mut repl::State,
    statement: &str)
    -> anyhow::Result<()>
{
    use crate::repl::OutputMode::*;

    let statement_name = Bytes::from_static(b"");

    let mut headers = HashMap::new();
    if let Some(implicit_limit) = state.implicit_limit {
        headers.insert(
            QUERY_OPT_IMPLICIT_LIMIT,
            Bytes::from(format!("{}", implicit_limit+1)));
    }
    let cli = state.connection.as_mut().expect("connection established");

    let mut seq = cli.start_sequence().await?;
    seq.send_messages(&[
        ClientMessage::Prepare(Prepare {
            headers,
            io_format: match state.output_mode {
                Default | TabSeparated => IoFormat::Binary,
                Json => IoFormat::Json,
                JsonElements => IoFormat::JsonElements,
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
                print_query_error(&err, statement, state.verbose_errors)?;
                state.last_error = Some(err.into());
                seq.err_sync().await?;
                return Ok(());
            }
            _ => {
                eprintln!("WARNING: unsolicited message {:?}", msg);
            }
        }
    }

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
                eprintln!("{}", err.display(state.verbose_errors));
                state.last_error = Some(err.into());
                seq.expect_ready().await?;
                return Ok(());
            }
            _ => {
                eprintln!("WARNING: unsolicited message {:?}", msg);
            }
        }
    };
    if options.debug_print_descriptors {
        println!("Descriptor: {:?}", data_description);
    }
    let desc = data_description.output()?;
    let indesc = data_description.input()?;
    if options.debug_print_descriptors {
        println!("InputDescr {:#?}", indesc.descriptors());
        println!("Output Descr {:#?}", desc.descriptors());
    }
    let codec = desc.build_codec()?;
    if options.debug_print_codecs {
        println!("Codec {:#?}", codec);
    }
    let incodec = indesc.build_codec()?;
    if options.debug_print_codecs {
        println!("Input Codec {:#?}", incodec);
    }

    let input = match input_variables(&indesc, &mut state.prompt).await {
        Ok(input) => input,
        Err(e) => {
            eprintln!("{:#}", e);
            state.last_error = Some(e);
            seq.end_clean();
            return Ok(());
        }
    };

    let mut arguments = BytesMut::with_capacity(8);
    incodec.encode(&mut arguments, &input)?;

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
            }
        }
        return Ok(());
    }

    let mut cfg = state.print.clone();
    if let Some((w, _h)) = term_size::dimensions_stdout() {
        // update max_width each time
        cfg.max_width(w);
    }
    match state.output_mode {
        TabSeparated => {
            let mut index = 0;
            while let Some(row) = items.next().await.transpose()? {
                if let Some(limit) = state.implicit_limit {
                    if index >= limit {
                        eprintln!("ERROR: Too many rows. Consider \
                            putting an explicit LIMIT clause, \
                            or increase the implicit limit \
                            using `\\set limit`.");
                        items.skip_remaining().await?;
                        return Ok(());
                    }
                }
                let mut text = match tab_separated::format_row(&row) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        // exhaust the iterator to get connection in the
                        // consistent state
                        items.skip_remaining().await?;
                        return Ok(());
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
                            source: ReadError::RequestError {
                                ref error, ..},
                            ..
                        } => {
                            eprintln!("{}", error);
                        }
                        _ => eprintln!("{:#?}", e),
                    }
                    state.last_error = Some(e.into());
                    return Ok(());
                }
            }
            println!();
        }
        Json => {
            while let Some(row) = items.next().await.transpose()? {
                let text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "postres returned non-string in JSON mode")),
                };
                let jitems: serde_json::Value;
                jitems = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                if let Some(limit) = state.implicit_limit {
                    if !check_json_limit(&jitems, "", limit) {
                        items.skip_remaining().await?;
                        return Ok(());
                    }
                }
                let jitems = jitems.as_array()
                    .ok_or_else(|| anyhow::anyhow!(
                        "non-array returned from \
                         postgres in JSON mode"))?;
                // trying to make writes atomic if possible
                let mut data = print::json_to_string(jitems, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
            }
        }
        JsonElements => {
            let mut index = 0;
            while let Some(row) = items.next().await.transpose()? {
                let text = match row {
                    Value::Str(s) => s,
                    _ => return Err(anyhow::anyhow!(
                        "postgres returned non-string in JSON mode")),
                };
                let value: serde_json::Value;
                value = serde_json::from_str(&text)
                    .context("cannot decode json result")?;
                let path = format!(".[{}]", index);
                if let Some(limit) = state.implicit_limit {
                    if index >= limit {
                        print_json_limit_error(&path);
                        items.skip_remaining().await?;
                        return Ok(());
                    }
                    if !check_json_limit(&value, &path, limit) {
                        items.skip_remaining().await?;
                        return Ok(());
                    }
                }
                // trying to make writes atomic if possible
                let mut data;
                data = print::json_item_to_string(&value, &cfg)?;
                data += "\n";
                stdout().write_all(data.as_bytes()).await?;
                index += 1;
            }
        }
    }
    state.last_error = None;
    return Ok(());
}

async fn _interactive_main(options: &Options, state: &mut repl::State)
    -> Result<(), anyhow::Error>
{
    loop {
        state.ensure_connection().await?;
        let cur_initial = replace(&mut state.initial_text, String::new());
        let inp = match state.edgeql_input(&cur_initial).await {
            prompt::Input::Eof => {
                state.terminate().await;
                return Err(CleanShutdown)?;
            }
            prompt::Input::Interrupt => {
                if state.in_transaction() {
                    eprintln!("WARNING: Transaction cancelled")
                }
                state.reconnect().await?;
                continue;
            }
            prompt::Input::Text(inp) => inp,
        };
        if !state.in_transaction() {
            state.ensure_connection().await?;
        }
        for item in ToDo::new(&inp) {
            let result = match item {
                ToDoItem::Backslash(text) => {
                    execute_backslash(state, text).await
                }
                ToDoItem::Query(statement) => {
                    execute_query(options, state, statement).await
                }
            };
            if let Err(err) = result {
                eprintln!("Error: {:#}", err);
                if state.connection.as_ref().map(|c| !c.is_consistent())
                    .unwrap_or(true)
                {
                    // Don't continue next statements on error
                    break;
                }
            }
        }
    }
}
