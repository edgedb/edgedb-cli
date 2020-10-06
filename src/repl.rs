use async_std::sync::{Sender, Receiver, RecvError};

use async_std::prelude::FutureExt;
use colorful::Colorful;
use edgedb_protocol::server_message::TransactionState;

use edgedb_client::{self as client, client::Connection};
use crate::prompt;
use crate::print;


pub const TX_MARKER: &str = "[tx]";
pub const FAILURE_MARKER: &str = "[tx:failed]";


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Default,
    Json,
    JsonElements,
    TabSeparated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Vi,
    Emacs,
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
    pub output_mode: OutputMode,
    pub history_limit: usize,
    pub conn_params: client::Builder,
    pub database: String,
    pub connection: Option<Connection>,
    pub last_version: Option<String>,
    pub initial_text: String,
}

impl PromptRpc {
    pub async fn variable_input(&mut self,
        name: &str, type_name: &str, initial: &str)
        -> prompt::Input
    {
        self.control.send(
            prompt::Control::ParameterInput {
                name: name.to_owned(),
                type_name: type_name.to_owned(),
                initial: initial.to_owned(),
            }
        ).await;
        match self.data.recv().await {
            Err(RecvError) | Ok(prompt::Input::Eof) => prompt::Input::Eof,
            Ok(x) => x,
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
        self.database = self.conn_params.get_effective_database();
        self.connection = Some(conn);
        Ok(())
    }
    pub async fn try_connect(&mut self, database: &str) -> anyhow::Result<()> {
        let mut params = self.conn_params.clone();
        params.database(database);
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
                conn.terminate().await
                    .map_err(|e| log::warn!("Termination error: {:#}", e))
                    .ok();
            }
        }
    }
    pub async fn edgeql_input(&mut self, initial: &str) -> prompt::Input {
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
        ).await;
        let result = if let Some(conn) = &mut self.connection {
            self.prompt.data.recv().race(conn.passive_wait()).await
        } else {
            self.prompt.data.recv().await
        };
        match result {
            Err(RecvError) | Ok(prompt::Input::Eof) => prompt::Input::Eof,
            Ok(x) => x,
        }
    }
    pub async fn input_mode(&mut self, value: InputMode) {
        self.input_mode = value;
        let msg = match value {
            InputMode::Vi => prompt::Control::ViMode,
            InputMode::Emacs => prompt::Control::EmacsMode,
        };
        self.prompt.control.send(msg).await
    }
    pub async fn show_history(&self) {
        self.prompt.control.send(prompt::Control::ShowHistory).await;
    }
    pub async fn spawn_editor(&self, entry: Option<isize>) -> prompt::Input {
        self.prompt.control.send(prompt::Control::SpawnEditor { entry }).await;
        match self.prompt.data.recv().await {
            Err(RecvError) | Ok(prompt::Input::Eof) => prompt::Input::Eof,
            Ok(x) => x,
        }
    }
    pub async fn set_history_limit(&mut self, val: usize) {
        self.history_limit = val;
        self.prompt.control.send(prompt::Control::SetHistoryLimit(val)).await;
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
            "input-mode" => Ok(InputMode::Emacs),
            _ => Err(anyhow::anyhow!("unsupported input mode {:?}", s)),
        }
    }
}

impl std::str::FromStr for OutputMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<OutputMode, anyhow::Error> {
        match s {
            "json" => Ok(OutputMode::Json),
            "json-elements" => Ok(OutputMode::JsonElements),
            "tab-separated" => Ok(OutputMode::TabSeparated),
            "default" => Ok(OutputMode::Default),
            _ => Err(anyhow::anyhow!("unsupported output mode {:?}", s)),
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

impl OutputMode {
    pub fn as_str(&self) -> &'static str {
        use OutputMode::*;
        match self {
            Default => "default",
            Json => "json",
            JsonElements => "json-elements",
            TabSeparated => "tab-separated",
        }
    }
}
