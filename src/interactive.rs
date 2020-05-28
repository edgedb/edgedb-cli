use std::collections::HashMap;
use std::io;
use std::mem::replace;
use std::str;

use anyhow::{self, Context};
use async_std::task;
use async_std::prelude::StreamExt;
use async_std::io::stdout;
use async_std::io::prelude::WriteExt;
use async_std::sync::{channel};
use colorful::Colorful;
use bytes::{Bytes, BytesMut};

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

use crate::client::{Connection, Client};


const QUERY_OPT_IMPLICIT_LIMIT: u16 = 0xFF01;


pub fn main(options: Options) -> Result<(), anyhow::Error> {
    let (control_wr, control_rd) = channel(1);
    let (repl_wr, repl_rd) = channel(1);
    let state = repl::State {
        control: control_wr,
        data: repl_rd,
        print: print::Config::new()
            .max_items(100)
            .colors(atty::is(atty::Stream::Stdout))
            .clone(),
        verbose_errors: false,
        last_error: None,
        database: options.database.clone(),
        implicit_limit: Some(100),
        output_mode: options.output_mode,
        input_mode: repl::InputMode::Emacs,
        history_limit: 100,
    };
    let handle = task::spawn(_main(options, state));
    prompt::main(repl_wr, control_rd)?;
    task::block_on(handle)?;
    Ok(())
}

pub async fn _main(options: Options, mut state: repl::State)
    -> anyhow::Result<()>
{
    let mut banner = false;
    let mut version = None;
    loop {
        let mut conn = Connection::from_options(&options).await?;
        let mut cli = conn.authenticate(&options, &state.database).await?;
        let fetched_version = cli.get_version().await?;
        if !banner || version.as_ref() != Some(&fetched_version) {
            println!("{} {}",
                "EdgeDB".light_gray(),
                fetched_version[..].light_gray());
            version = Some(fetched_version);
        }
        if !banner {
            println!("{}", r#"Type "\?" for help."#.light_gray());
            banner = true;
        }
        match _interactive_main(cli, &options, &mut state).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if let Some(err) = e.downcast_ref::<backslash::ChangeDb>() {
                    state.database = err.target.clone();
                    continue;
                }
                if let Some(err) = e.downcast_ref::<ReadError>() {
                    match err {
                        ReadError::Eos => {
                            eprintln!("Connection is broken. Reconnecting...");
                            continue;
                        }
                        _ => {}
                    }
                }
                if let Some(err) = e.downcast_ref::<io::Error>() {
                    if err.kind() == io::ErrorKind::BrokenPipe {
                        eprintln!("Connection is broken. Reconnecting...");
                        continue;
                    }
                }
                return Err(e);
            }
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

async fn _interactive_main(
    mut cli: Client<'_>, options: &Options, mut state: &mut repl::State)
    -> Result<(), anyhow::Error>
{
    use crate::repl::OutputMode::*;
    let mut initial = String::new();
    let statement_name = Bytes::from_static(b"");

    loop {
        let inp = match
            state.edgeql_input(&replace(&mut initial, String::new())).await
        {
            prompt::Input::Eof => {
                cli.send_messages(&[ClientMessage::Terminate]).await?;
                match cli.reader.message().await {
                    Err(ReadError::Eos) => {}
                    Err(e) => {
                        eprintln!("WARNING: error on terminate: {}", e);
                    }
                    Ok(msg) => {
                        eprintln!("WARNING: unsolicited message {:?}", msg);
                    }
                }
                return Ok(());
            }
            prompt::Input::Interrupt => continue,
            prompt::Input::Text(inp) => inp,
        };
        if inp.trim().is_empty() {
            continue;
        }
        let mut current_offset = 0;
        'statement_loop: while inp[current_offset..].trim() != "" {
            let inp_tail = &inp[current_offset..].trim_start();
            current_offset = inp.len() - inp_tail.len();
            if inp_tail.starts_with("\\") {
                use backslash::ExecuteResult::*;
                let len = backslash::full_statement(&inp_tail);
                current_offset += len;
                let cmd = match backslash::parse(&inp_tail[..len]) {
                    Ok(cmd) => cmd,
                    Err(e) => {
                        eprintln!("Error parsing backslash command: {}",
                                  e.message);
                        if inp_tail[len..].trim().is_empty() {
                            // Quick-edit command on error
                            initial = inp_tail[..len].trim_start().into();
                        }
                        continue 'statement_loop;
                    }
                };
                let res = backslash::execute(&mut cli,
                    &cmd.command, &mut state).await;
                match res {
                    Ok(Skip) => continue,
                    Ok(Quit) => return Ok(()),
                    Ok(Input(text)) => initial = text,
                    Err(e) => {
                        if e.is::<backslash::ChangeDb>() {
                            if !inp_tail[len..].trim().is_empty() {
                                eprintln!("WARNING: subsequent commands after \
                                           \\connect are ignored");
                            }
                            return Err(e);
                        }
                        eprintln!("Error executing command: {}", e);
                        // Quick-edit command on error
                        initial = inp.trim_start().into();
                        state.last_error = Some(e);
                    }
                }
                continue 'statement_loop;
            }
            let slen = full_statement(&inp_tail.as_bytes(), None)
                .unwrap_or(inp_tail.len());
            let statement = &inp_tail[..slen];
            current_offset += slen;
            let mut headers = HashMap::new();
            if let Some(implicit_limit) = state.implicit_limit {
                headers.insert(
                    QUERY_OPT_IMPLICIT_LIMIT,
                    Bytes::from(format!("{}", implicit_limit+1)));
            }

            cli.send_messages(&[
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
                ClientMessage::Sync,
            ]).await?;

            loop {
                let msg = cli.reader.message().await?;
                match msg {
                    ServerMessage::PrepareComplete(..) => {
                        cli.reader.wait_ready().await?;
                        break;
                    }
                    ServerMessage::ErrorResponse(err) => {
                        print_query_error(&err, statement, state.verbose_errors)?;
                        state.last_error = Some(err.into());
                        cli.reader.wait_ready().await?;
                        continue 'statement_loop;
                    }
                    _ => {
                        eprintln!("WARNING: unsolicited message {:?}", msg);
                    }
                }
            }

            cli.send_messages(&[
                ClientMessage::DescribeStatement(DescribeStatement {
                    headers: HashMap::new(),
                    aspect: DescribeAspect::DataDescription,
                    statement_name: statement_name.clone(),
                }),
                ClientMessage::Flush,
            ]).await?;

            let data_description = loop {
                let msg = cli.reader.message().await?;
                match msg {
                    ServerMessage::CommandDataDescription(data_desc) => {
                        break data_desc;
                    }
                    ServerMessage::ErrorResponse(err) => {
                        eprintln!("{}", err.display(state.verbose_errors));
                        state.last_error = Some(err.into());
                        cli.reader.wait_ready().await?;
                        continue 'statement_loop;
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

            let input = match input_variables(&indesc, state).await {
                Ok(input) => input,
                Err(e) => {
                    eprintln!("{:#}", e);
                    state.last_error = Some(e);
                    continue 'statement_loop;
                }
            };

            let mut arguments = BytesMut::with_capacity(8);
            incodec.encode(&mut arguments, &input)?;

            cli.send_messages(&[
                ClientMessage::Execute(Execute {
                    headers: HashMap::new(),
                    statement_name: statement_name.clone(),
                    arguments: arguments.freeze(),
                }),
                ClientMessage::Sync,
            ]).await?;

            let mut items = cli.reader.response(codec);
            if desc.root_pos().is_none() {
                match cli._process_exec().await {
                    Ok(ref val) => print::completion(val),
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        state.last_error = Some(e.into());
                        cli.reader.wait_ready().await?;
                    }
                }
                continue 'statement_loop;
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
                                continue 'statement_loop;
                            }
                        }
                        let mut text = match tab_separated::format_row(&row) {
                            Ok(text) => text,
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                // exhaust the iterator to get connection in the
                                // consistent state
                                items.skip_remaining().await?;
                                continue 'statement_loop;
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
                            cli.reader.wait_ready().await?;
                            continue 'statement_loop;
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
                                continue 'statement_loop;
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
                                "postres returned non-string in JSON mode")),
                        };
                        let value: serde_json::Value;
                        value = serde_json::from_str(&text)
                            .context("cannot decode json result")?;
                        let path = format!(".[{}]", index);
                        if let Some(limit) = state.implicit_limit {
                            if index >= limit {
                                print_json_limit_error(&path);
                                items.skip_remaining().await?;
                                continue 'statement_loop;
                            }
                            if !check_json_limit(&value, &path, limit) {
                                items.skip_remaining().await?;
                                continue 'statement_loop;
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
        }
    }
}
