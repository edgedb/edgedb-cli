use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_std::prelude::FutureExt;
use async_std::channel::{Sender, Receiver, RecvError};
use colorful::Colorful;
use edgedb_client::client::Connection;
use edgedb_protocol::server_message::TransactionState;

use crate::async_util::timeout;
use crate::connect::Connector;
use crate::print;
use crate::prompt::variable::VariableInput;
use crate::prompt;


pub const TX_MARKER: &str = "[tx]";
pub const FAILURE_MARKER: &str = "[tx:failed]";


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Default,
    Json,
    JsonPretty,
    JsonLines,
    TabSeparated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Vi,
    Emacs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintStats {
    Off,
    Query,
    Detailed,
}


pub struct PromptRpc {
    pub control: Sender<prompt::Control>,
    pub data: Receiver<prompt::Input>,
}

pub struct State {
    pub prompt: PromptRpc,
    pub print: print::Config,
    pub verbose_errors: bool,
    pub last_error: Option<anyhow::Error>,
    pub implicit_limit: Option<usize>,
    pub input_mode: InputMode,
    pub output_format: OutputFormat,
    pub print_stats: PrintStats,
    pub history_limit: usize,
    pub conn_params: Connector,
    pub database: String,
    pub connection: Option<Connection>,
    pub last_version: Option<String>,
    pub initial_text: String,
}

impl PromptRpc {
    pub async fn variable_input(&mut self,
        name: &str, var_type: Arc<dyn VariableInput>, optional: bool,
        initial: &str)
        -> anyhow::Result<prompt::Input>
    {
        self.control.send(
                prompt::Control::ParameterInput {
                    name: name.to_owned(),
                    var_type,
                    optional,
                    initial: initial.to_owned(),
                }
            ).await
            .context("cannot send to input thread")?;
        match self.data.recv().await {
            Err(RecvError) | Ok(prompt::Input::Eof) => Ok(prompt::Input::Eof),
            Ok(x) => Ok(x),
        }
    }
}

impl State {
    pub async fn reconnect(&mut self) -> anyhow::Result<()> {
        let mut conn = self.conn_params.connect().await?;
        let fetched_version = conn.get_version().await?;
        if self.last_version.as_ref() != Some(&fetched_version) {
            println!("{} {} (repl v{})",
                "EdgeDB".light_gray(),
                fetched_version[..].light_gray(),
                env!("CARGO_PKG_VERSION"));
            self.last_version = Some(fetched_version);
        }
        self.database = self.conn_params.get()?.get_database().into();
        self.connection = Some(conn);
        Ok(())
    }
    pub async fn try_connect(&mut self, database: &str) -> anyhow::Result<()> {
        let mut params = self.conn_params.clone();
        params.modify(|p| { p.database(database); });
        let mut conn = params.connect().await?;
        let fetched_version = conn.get_version().await?;
        if self.last_version.as_ref() != Some(&fetched_version) {
            println!("{} {} (repl v{})",
                "EdgeDB".light_gray(),
                fetched_version[..].light_gray(),
                env!("CARGO_PKG_VERSION"));
            self.last_version = Some(fetched_version);
        }
        self.conn_params = params;
        self.database = database.into();
        self.connection = Some(conn);
        Ok(())
    }
    pub async fn soft_reconnect(&mut self) -> anyhow::Result<()> {
        if !self.in_transaction() {
            self.ensure_connection().await?;
        }
        Ok(())
    }
    pub async fn ensure_connection(&mut self) -> anyhow::Result<()> {
        match &self.connection {
            Some(c) if c.is_consistent() => {}
            Some(_) => {
                eprintln!("Reconnecting...");
                self.reconnect().await?;
            }
            None => {
                self.reconnect().await?;
            }
        };
        Ok(())
    }
    pub async fn terminate(&mut self) {
        if let Some(conn) = self.connection.take() {
            if conn.is_consistent() {
                timeout(Duration::from_secs(1), conn.terminate()).await
                    .map_err(|e| log::warn!("Termination error: {:#}", e))
                    .ok();
            }
        }
    }
    pub async fn edgeql_input(&mut self, initial: &str)
        -> anyhow::Result<prompt::Input>
    {
        use TransactionState::*;

        let prompt = format!("{}{}> ",
            self.database,
            match self.connection.as_ref().map(|c| c.transaction_state()) {
                Some(NotInTransaction) => "",
                Some(InTransaction) => TX_MARKER,
                Some(InFailedTransaction) => FAILURE_MARKER,
                None => "",
            }
        );
        self.prompt.control.send(
                prompt::Control::EdgeqlInput {
                    prompt,
                    initial: initial.to_owned(),
                }
            ).await
            .context("cannot send to input thread")?;
        let result = if let Some(conn) = &mut self.connection {
            self.prompt.data.recv().race(conn.passive_wait()).await
        } else {
            self.prompt.data.recv().await
        };
        match result {
            Err(RecvError) | Ok(prompt::Input::Eof) => Ok(prompt::Input::Eof),
            Ok(x) => Ok(x),
        }
    }
    pub async fn input_mode(&mut self, value: InputMode) -> anyhow::Result<()>
    {
        self.input_mode = value;
        let msg = match value {
            InputMode::Vi => prompt::Control::ViMode,
            InputMode::Emacs => prompt::Control::EmacsMode,
        };
        self.prompt.control.send(msg).await
            .context("cannot send to input thread")
    }
    pub async fn show_history(&self) -> anyhow::Result<()> {
        self.prompt.control.send(prompt::Control::ShowHistory).await
            .context("cannot send to input thread")
    }
    pub async fn spawn_editor(&self, entry: Option<isize>)
        -> anyhow::Result<prompt::Input>
    {
        self.prompt.control.send(prompt::Control::SpawnEditor { entry }).await
            .context("cannot send to input thread")?;
        match self.prompt.data.recv().await {
            Err(RecvError) | Ok(prompt::Input::Eof) => Ok(prompt::Input::Eof),
            Ok(x) => Ok(x),
        }
    }
    pub async fn set_history_limit(&mut self, val: usize)
        -> anyhow::Result<()>
    {
        self.history_limit = val;
        self.prompt.control.send(prompt::Control::SetHistoryLimit(val)).await
            .context("cannot send to input thread")
    }
    pub fn in_transaction(&self) -> bool {
        match &self.connection {
            Some(conn) => {
                matches!(conn.transaction_state(),
                         TransactionState::InTransaction)
            }
            None => false,
        }
    }
}

impl std::str::FromStr for InputMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<InputMode, anyhow::Error> {
        match s {
            "vi" => Ok(InputMode::Vi),
            "emacs" => Ok(InputMode::Emacs),
            _ => Err(anyhow::anyhow!("unsupported input mode {:?}", s)),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<OutputFormat, anyhow::Error> {
        match s {
            "json" => Ok(OutputFormat::Json),
            "json-pretty" => Ok(OutputFormat::JsonPretty),
            "json-lines" => Ok(OutputFormat::JsonLines),
            "tab-separated" => Ok(OutputFormat::TabSeparated),
            "default" => Ok(OutputFormat::Default),
            _ => Err(anyhow::anyhow!("unsupported output mode {:?}", s)),
        }
    }
}

impl std::str::FromStr for PrintStats {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<PrintStats, anyhow::Error> {
        match s {
            "off" => Ok(PrintStats::Off),
            "query" => Ok(PrintStats::Query),
            "detailed" => Ok(PrintStats::Detailed),
            _ => Err(anyhow::anyhow!("unsupported stats mode {:?}", s)),
        }
    }
}


impl InputMode {
    pub fn as_str(&self) -> &'static str {
        use InputMode::*;
        match self {
            Vi => "vi",
            Emacs => "emacs",
        }
    }
}

impl OutputFormat {
    pub fn as_str(&self) -> &'static str {
        use OutputFormat::*;
        match self {
            Default => "default",
            Json => "json",
            JsonPretty => "json-pretty",
            JsonLines => "json-lines",
            TabSeparated => "tab-separated",
        }
    }
}

impl PrintStats {
    pub fn as_str(&self) -> &'static str {
        use PrintStats::*;
        match self {
            Off => "off",
            Query => "query",
            Detailed => "detailed",
        }
    }
}
