use std::mem::replace;
use std::str;
use std::time::Instant;

use anyhow::{self, Context};
use colorful::Colorful;
use is_terminal::IsTerminal;
use terminal_size::{Width, terminal_size};
use tokio::io::{stdout, AsyncWriteExt};
use tokio::sync::mpsc::channel;
use tokio_stream::StreamExt;

use edgedb_protocol::client_message::{CompilationOptions};
use edgedb_protocol::client_message::{IoFormat, Cardinality};
use edgedb_protocol::common::{Capabilities, State};
use edgedb_protocol::common::{RawTypedesc};
use edgedb_protocol::model::Duration;
use edgedb_protocol::value::Value;
use edgedb_errors::StateMismatchError;
use edgeql_parser::preparser::{self, full_statement};

use crate::analyze;
use crate::classify;
use crate::commands::{backslash, ExitCode};
use crate::config::Config;
use crate::credentials;
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


#[derive(Debug, thiserror::Error)]
#[error("Shutting down on user request")]
pub struct CleanShutdown;

#[derive(Debug, thiserror::Error)]
#[error("QueryError")]
pub struct QueryError;

#[derive(Debug, thiserror::Error)]
#[error("RetryStateError")]
pub struct RetryStateError;

struct ToDo<'a> {
    tail: &'a str,
}

#[derive(Debug, PartialEq)]
pub enum ToDoItem<'a> {
    Query(&'a str),
    Explain(&'a str),
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
                let query = &tail[..len];
                self.tail = &tail[len..];
                if preparser::is_empty(query) {
                    continue;
                }
                if classify::is_analyze(query) {
                    return Some(ToDoItem::Explain(query))
                } else {
                    return Some(ToDoItem::Query(query))
                }
            }
        }
    }
}

pub fn main(options: Options, cfg: Config) -> Result<(), anyhow::Error> {
    let (control_wr, control_rd) = channel(1);
    let conn = options.block_on_create_connector()?;
    let limit = cfg.shell.limit.unwrap_or(100);
    let implicit_limit = if limit != 0 { Some(limit) } else { None };
    let idle_tx_timeout = cfg.shell.idle_transaction_timeout
        .unwrap_or_else(|| Duration::from_micros(5 * 60_000_000));
    let print = print::Config::new()
        .max_items(implicit_limit)
        .expand_strings(cfg.shell.expand_strings.unwrap_or(true))
        .implicit_properties(cfg.shell.implicit_properties.unwrap_or(false))
        .colors(std::io::stdout().is_terminal())
        .clone();
    let conn_config = conn.get()?;
    credentials::maybe_update_credentials_file(conn_config, true)?;
    let state = repl::State {
        prompt: repl::PromptRpc {
            control: control_wr,
        },
        print,
        verbose_errors: cfg.shell.verbose_errors.unwrap_or(false),
        last_error: None,
        last_analyze: None,
        implicit_limit: implicit_limit,
        idle_transaction_timeout: idle_tx_timeout,
        output_format: options.output_format
            .or(cfg.shell.output_format)
            .unwrap_or(repl::OutputFormat::Default),
        display_typenames: cfg.shell.display_typenames.unwrap_or(true),
        input_mode: cfg.shell.input_mode.unwrap_or(repl::InputMode::Emacs),
        print_stats: cfg.shell.print_stats.unwrap_or(repl::PrintStats::Off),
        history_limit: cfg.shell.history_size.unwrap_or(10000),
        database: conn_config.database().into(),
        conn_params: conn,
        last_version: None,
        connection: None,
        initial_text: "".into(),
        edgeql_state_desc: RawTypedesc::uninitialized(),
        edgeql_state: State::empty(),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let handle = runtime.spawn(_main(options, state, cfg));
    prompt::main(control_rd)?;
    runtime.block_on(handle)??;
    Ok(())
}

pub async fn _main(options: Options, mut state: repl::State, cfg: Config)
    -> anyhow::Result<()>
{
    let mut conn = state.conn_params.connect().await?;
    let fetched_version = conn.get_version().await?;
    echo!("EdgeDB".light_gray(), fetched_version.to_string().light_gray(),
        format_args!("(repl {})", env!("CARGO_PKG_VERSION")).fade());
    state.last_version = Some(fetched_version.clone());
    if let Some(config_path) = &cfg.file_name {
        echo!(
            format_args!("Applied {} configuration file",
                config_path.display(),
            ).fade());
    }
    echo!(r#"Type \help for help, \quit to quit."#.light_gray());
    state.set_history_limit(state.history_limit).await?;
    state.connection = Some(conn);
    state.set_idle_transaction_timeout().await?;
    state.read_state();
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

async fn write_out(data: &str) -> anyhow::Result<()> {
    let mut out = stdout();
    out.write_all(data.as_bytes()).await?;
    out.flush().await?;
    Ok(())
}

async fn execute_query(options: &Options, state: &mut repl::State,
    statement: &str)
    -> anyhow::Result<()>
{
    use crate::repl::OutputFormat::*;
    use crate::repl::PrintStats::*;

    let cli = state.connection.as_mut().expect("connection established");
    let flags = CompilationOptions {
        implicit_limit: state.implicit_limit.map(|x| (x+1) as u64),
        implicit_typenames: state.display_typenames &&
            cli.protocol().supports_inline_typenames(),
        implicit_typeids: false,
        explicit_objectids: true,
        allow_capabilities: Capabilities::ALL,
        io_format: match state.output_format {
            Default | TabSeparated => IoFormat::Binary,
            JsonLines | JsonPretty => IoFormat::JsonElements,
            Json => IoFormat::Json,
        },
        expected_cardinality: Cardinality::Many,
    };

    let start = Instant::now();
    let data_description = match cli.parse(&flags, statement).await {
        Ok(desc) => desc,
        Err(e) if e.is::<StateMismatchError>() => return Err(RetryStateError)?,
        Err(e) => {
            print_query_error(&e, statement, state.verbose_errors, "<query>")?;
            return Err(QueryError)?;
        }
    };
    let first_part = start.elapsed();

    if state.print_stats == Detailed {
        eprintln!("{}",
            format!("Parse: {:?}", start.elapsed()).dark_gray());
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

    let input = match
        cli.ping_while(input_variables(&indesc, &mut state.prompt)).await
    {
        Ok(input) => input,
        Err(e) => {
            eprintln!("{:#}", e);
            state.last_error = Some(e);
            return Err(QueryError)?;
        }
    };

    let start_execute = Instant::now();
    let mut items = cli.execute_stream(
        &flags, statement, &data_description, &input,
    ).await?;

    if !items.can_contain_data() {
        match items.complete().await {
            Ok(res) => print::completion(&res.status_data),
            Err(e) if e.is::<StateMismatchError>() => {
                return Err(RetryStateError)?;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                state.last_error = Some(e.into());
                return Err(QueryError)?;
            }
        }
        return Ok(());
    }

    let mut cfg = state.print.clone();
    if let Some((Width(w), _h)) = terminal_size() {
        // update max_width each time
        cfg.max_width(w.into());
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
                        items.complete().await?;
                        return Err(QueryError)?;
                    }
                }
                let mut text = match tab_separated::format_row(&row) {
                    Ok(text) => text,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        // exhaust the iterator to get connection in the
                        // consistent state
                        items.complete().await?;
                        return Err(QueryError)?;
                    }
                };
                // trying to make writes atomic if possible
                text += "\n";
                write_out(&text).await?;
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
                            print_query_error(
                                error, statement, state.verbose_errors,
                                "<query>",
                            )?;
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
                        items.complete().await?;
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
                write_out(&data).await?;
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
                        items.complete().await?;
                        return Err(QueryError)?;
                    }
                    if !check_json_limit(&value, &path, limit) {
                        items.complete().await?;
                        return Err(QueryError)?;
                    }
                }
                if state.output_format == JsonLines {
                    // trying to make writes atomic if possible
                    text += "\n";
                    write_out(&text).await?;
                } else {
                    // trying to make writes atomic if possible
                    let mut data;
                    data = print::json_item_to_string(&value, &cfg)?;
                    data += "\n";
                    write_out(&data).await?;
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
        tokio::select!(
            _ = state.ensure_connection() => {}
            res = ctrlc.wait_result() => res?,
        );
        let cur_initial = replace(&mut state.initial_text, String::new());
        let inp = match state.edgeql_input(&cur_initial).await? {
            prompt::Input::Eof => {
                tokio::select!(
                    _ = state.terminate() => {}
                    _ = ctrlc.wait() => {}
                );
                return Err(CleanShutdown)?;
            }
            prompt::Input::Interrupt => {
                continue;
            }
            prompt::Input::Text(inp) => inp,
        };
        'todo: for item in ToDo::new(&inp) {
            'retry: loop {
                let result = match item {
                    ToDoItem::Backslash(text) => {
                        tokio::select!(
                            res = execute_backslash(state, text) => res,
                            res = ctrlc.wait_result() => res,
                        )
                    }
                    ToDoItem::Explain(statement) => {
                        tokio::select!(
                            r = state.soft_reconnect() => r,
                            r = ctrlc.wait_result() => r,
                        ).and(tokio::select!(
                            r = analyze::interactive(state, statement) => r,
                            r = ctrlc.wait_result() => r,
                        ))
                    }
                    ToDoItem::Query(statement) => {
                        tokio::select!(
                            r = state.soft_reconnect() => r,
                            r = ctrlc.wait_result() => r,
                        ).and(tokio::select!(
                            r = execute_query(options, state, statement) => r,
                            r = ctrlc.wait_result() => r,
                        ))
                    }
                };
                if let Err(err) = result {
                    if err.is::<InterruptError>() {
                        eprintln!("Interrupted.");
                        tokio::select!(
                            _ = state.reconnect() => {}
                            r = ctrlc.wait_result() => r?,
                        );
                    } else if err.is::<CleanShutdown>() {
                        return Err(err)?;
                    } else if err.is::<RetryStateError>() {
                        if state.try_update_state()? {
                            continue 'retry;
                        }
                        print::error(
                            "State could not be updated automatically");
                        echo!("  Hint: This means some migrations or DDL \
                               statements were run in a concurrent \
                               connection during your interactive \
                               session. Restarting CLI usually works \
                               (although, you have to to set globals \
                               and aliases again)");
                        return Err(ExitCode::new(10))?;
                    } else if let
                        Some(e) = err.downcast_ref::<edgedb_errors::Error>()
                    {
                        print::edgedb_error(e, state.verbose_errors);
                    } else if !err.is::<QueryError>() {
                        print::error(err);
                    }
                    // Don't continue next statements on error
                    break 'todo;
                }
                state.read_state();
                // only retry on StateMismatchError
                break 'retry;
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
