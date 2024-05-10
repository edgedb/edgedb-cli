use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bytes::BytesMut;
use colorful::Colorful;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;

use edgedb_errors::{ClientError, ProtocolEncodingError};
use edgedb_errors::{Error, ErrorKind};
use edgedb_protocol::common::{RawTypedesc, State as EdgeqlState};
use edgedb_protocol::model::Duration as EdbDuration;
use edgedb_protocol::model::Uuid;
use edgedb_protocol::server_message::TransactionState;
use edgedb_protocol::value::Value;

use crate::analyze;
use crate::async_util::timeout;
use crate::connect::Connection;
use crate::connect::Connector;
use crate::echo;
use crate::portable::ver;
use crate::print::{self, Highlight};
use crate::prompt::variable::VariableInput;
use crate::prompt::{self, Control};

pub const TX_MARKER: &str = "[tx]";
pub const FAILURE_MARKER: &str = "[tx:failed]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum OutputFormat {
    Default,
    Json,
    JsonPretty,
    JsonLines,
    TabSeparated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum InputMode {
    Vi,
    Emacs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum PrintStats {
    Off,
    Query,
    Detailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorLimit {
    Unlimited,
    Auto,
    Fixed(usize),
}

pub struct PromptRpc {
    pub control: Sender<Control>,
}

pub struct LastAnalyze {
    pub query: String,
    pub output: analyze::Analysis,
}

pub struct State {
    pub prompt: PromptRpc,
    pub print: print::Config,
    pub verbose_errors: bool,
    pub last_error: Option<anyhow::Error>,
    pub last_analyze: Option<LastAnalyze>,
    pub implicit_limit: Option<usize>,
    pub idle_transaction_timeout: EdbDuration,
    pub input_mode: InputMode,
    pub output_format: OutputFormat,
    pub display_typenames: bool,
    pub print_stats: PrintStats,
    pub history_limit: usize,
    pub conn_params: Connector,
    pub branch: String,
    pub connection: Option<Connection>,
    pub last_version: Option<ver::Build>,
    pub initial_text: String,
    pub edgeql_state_desc: RawTypedesc,
    pub edgeql_state: EdgeqlState,
    pub current_branch: Option<String>,
}

impl PromptRpc {
    pub async fn variable_input(
        &mut self,
        name: &str,
        var_type: Arc<dyn VariableInput>,
        optional: bool,
        initial: &str,
    ) -> anyhow::Result<prompt::VarInput> {
        let (response, rx) = oneshot::channel();
        self.control
            .send(prompt::Control::ParameterInput {
                name: name.to_owned(),
                var_type,
                optional,
                initial: initial.to_owned(),
                response,
            })
            .await
            .ok()
            .context("cannot send command to prompt thread")?;
        let res = rx
            .await
            .ok()
            .context("cannot get response from the prompt thread")?;
        Ok(res)
    }
}

impl State {
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let branch = self.conn_params.get()?.branch().to_owned();
        self.try_connect(&branch).await?;
        Ok(())
    }
    pub async fn reconnect(&mut self) -> anyhow::Result<()> {
        let branch = self.conn_params.get()?.branch().to_owned();
        let cur_state = self.edgeql_state.clone();
        let cur_state_desc = self.edgeql_state_desc.clone();
        self.try_connect(&branch).await?;
        if let Some(conn) = &mut self.connection {
            if cur_state_desc == self.edgeql_state_desc {
                conn.set_state(cur_state);
            } else {
                eprintln!("Discarding session configuration because server configuration layout has changed.");
            }
        }
        Ok(())
    }
    pub async fn set_idle_transaction_timeout(&mut self) -> anyhow::Result<()> {
        if let Some(conn) = &mut self.connection {
            if conn.protocol().is_at_least(0, 13) {
                let d = self.idle_transaction_timeout;
                log::info!("Setting session_idle_transaction_timeout to {}", d);
                conn.execute(
                    &format!(
                        "CONFIGURE SESSION SET session_idle_transaction_timeout \
                     := <std::duration>'{}us'",
                        d.to_micros(),
                    ),
                    &(),
                )
                .await
                .context("cannot configure session_idle_transaction_timeout")?;
            }
        }
        Ok(())
    }
    fn print_banner(&self, version: &ver::Build) -> anyhow::Result<()> {
        echo!(
            format!("{}\rEdgeDB", ansi_escapes::EraseLine).light_gray(),
            version.to_string().light_gray(),
            format_args!("(repl {})", env!("CARGO_PKG_VERSION")).fade(),
        );
        Ok(())
    }
    pub async fn try_connect(&mut self, branch: &str) -> anyhow::Result<()> {
        let mut params = self.conn_params.clone();
        params.branch(branch)?;
        let mut conn = params.connect_interactive().await?;
        let fetched_version = conn.get_version().await?;
        if self.last_version.as_ref() != Some(fetched_version) {
            self.print_banner(fetched_version)?;
            self.last_version = Some(fetched_version.to_owned());
        }
        self.conn_params = params;
        self.branch = branch.into();
        self.current_branch = Some(conn.get_current_branch().await?.to_string());
        self.connection = Some(conn);
        self.read_state();
        self.set_idle_transaction_timeout().await?;
        Ok(())
    }
    pub async fn soft_reconnect(&mut self) -> anyhow::Result<()> {
        if self.in_transaction() {
            let is_closed = self
                .connection
                .as_ref()
                .map(|c| !c.is_consistent())
                .unwrap_or(false);
            if is_closed {
                anyhow::bail!("connection closed by server");
            }
        } else {
            self.ensure_connection().await?;
        }
        Ok(())
    }
    pub async fn ensure_connection(&mut self) -> anyhow::Result<()> {
        match &self.connection {
            Some(c) if c.is_consistent() => return Ok(()),
            Some(_) => {
                eprintln!("Reconnecting...");
            }
            None => {}
        };
        self.reconnect().await?;
        Ok(())
    }
    pub async fn terminate(&mut self) {
        if let Some(conn) = self.connection.take() {
            if conn.is_consistent() {
                timeout(Duration::from_secs(1), conn.terminate())
                    .await
                    .map_err(|e| log::warn!("Termination error: {:#}", e))
                    .ok();
            }
        }
    }
    async fn editor_cmd<T>(
        &mut self,
        f: impl FnOnce(oneshot::Sender<T>) -> Control,
    ) -> anyhow::Result<T> {
        let (tx, rx) = oneshot::channel();
        let request = f(tx);
        if let Some(conn) = &mut self.connection {
            let prompt = &self.prompt;
            conn.ping_while(async {
                prompt
                    .control
                    .send(request)
                    .await
                    .ok()
                    .context("error sending command to prompt thread")?;
                anyhow::Ok(rx.await?)
            })
            .await
        } else {
            self.prompt
                .control
                .send(request)
                .await
                .ok()
                .context("error sending command to prompt thread")?;
            let res = rx
                .await
                .ok()
                .context("cannot get response from prompt thread")?;
            Ok(res)
        }
    }
    pub async fn edgeql_input(&mut self, initial: &str) -> anyhow::Result<prompt::Input> {
        use TransactionState::*;

        let txstate = match self.connection.as_mut().map(|c| c.transaction_state()) {
            Some(NotInTransaction) => "",
            Some(InTransaction) => TX_MARKER,
            Some(InFailedTransaction) => FAILURE_MARKER,
            None => "",
        };

        let current_database = match &self.current_branch {
            Some(db) => db,
            None => &self.branch,
        };

        let inst = self.conn_params.get()?.instance_name().to_owned();

        let location = match inst {
            Some(edgedb_tokio::InstanceName::Cloud {
                org_slug: org,
                name,
            }) => format!("{}/{}:{}", org, name, current_database,),
            Some(edgedb_tokio::InstanceName::Local(name)) => {
                format!("{}:{}", name, current_database,)
            }
            _ => format!("{}", current_database),
        };

        let prompt = format!("{}{}> ", location, txstate);

        self.editor_cmd(|response| prompt::Control::EdgeqlInput {
            prompt,
            initial: initial.to_owned(),
            response,
        })
        .await
    }

    pub async fn input_mode(&mut self, value: InputMode) -> anyhow::Result<()> {
        self.input_mode = value;
        let msg = match value {
            InputMode::Vi => prompt::Control::ViMode,
            InputMode::Emacs => prompt::Control::EmacsMode,
        };
        self.prompt
            .control
            .send(msg)
            .await
            .ok()
            .context("cannot send to input thread")
    }
    pub async fn show_history(&mut self) -> anyhow::Result<()> {
        self.editor_cmd(|ack| Control::ShowHistory { ack }).await
    }
    pub async fn spawn_editor(&mut self, entry: Option<isize>) -> anyhow::Result<prompt::Input> {
        self.editor_cmd(|response| Control::SpawnEditor { entry, response })
            .await
    }
    pub async fn set_history_limit(&mut self, val: usize) -> anyhow::Result<()> {
        self.history_limit = val;
        self.prompt
            .control
            .send(Control::SetHistoryLimit(val))
            .await
            .ok()
            .context("cannot send to input thread")
    }
    pub fn in_transaction(&self) -> bool {
        match &self.connection {
            Some(conn) => {
                matches!(conn.transaction_state(), TransactionState::InTransaction)
            }
            None => false,
        }
    }
    pub fn read_state(&mut self) {
        use TransactionState::NotInTransaction;

        if let Some(conn) = &self.connection {
            if matches!(conn.transaction_state(), NotInTransaction) {
                self.edgeql_state = conn.get_state().clone();
                self.edgeql_state_desc = conn.get_state_desc();
            }
        }
    }
    pub fn try_update_state(&mut self) -> anyhow::Result<bool> {
        if let Some(conn) = &mut self.connection {
            if !self.edgeql_state.data.is_empty() {
                let desc = self.edgeql_state_desc.decode()?;
                let codec = desc.build_codec()?;
                let value = codec.decode(&self.edgeql_state.data)?;

                let desc = conn.get_state_desc().decode()?;
                let codec = desc.build_codec()?;
                let mut buf = BytesMut::with_capacity(self.edgeql_state.data.len());
                codec.encode(&mut buf, &value)?;
                self.edgeql_state = EdgeqlState {
                    typedesc_id: desc.id().clone(),
                    data: buf.freeze(),
                };
                conn.set_state(self.edgeql_state.clone());
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn get_state_as_value(&self) -> Result<(Uuid, Value), Error> {
        if self.edgeql_state.typedesc_id == Uuid::from_u128(0) {
            return Ok((Uuid::from_u128(0), Value::Nothing));
        }
        let desc = &self.edgeql_state_desc;
        if desc.id != self.edgeql_state.typedesc_id {
            return Err(ClientError::with_message(format!(
                "State type descriptor id is {:?}, \
                             but state is encoded using {:?}",
                desc.id, self.edgeql_state.typedesc_id
            )));
        }
        let desc = desc.decode().map_err(ProtocolEncodingError::with_source)?;
        let codec = desc
            .build_codec()
            .map_err(ProtocolEncodingError::with_source)?;
        let value = codec
            .decode(&self.edgeql_state.data[..])
            .map_err(ProtocolEncodingError::with_source)?;

        Ok((desc.id().clone(), value))
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

impl std::str::FromStr for VectorLimit {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<VectorLimit, Self::Err> {
        match s {
            "unlimited" => Ok(VectorLimit::Unlimited),
            "auto" => Ok(VectorLimit::Auto),
            _ => s
                .parse()
                .map(VectorLimit::Fixed)
                .map_err(|_| "expected integer, `unlimited` or `auto`"),
        }
    }
}

impl fmt::Display for VectorLimit {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use VectorLimit::*;

        match self {
            Unlimited => "unlimited".fmt(f),
            Auto => "auto".fmt(f),
            Fixed(x) => x.fmt(f),
        }
    }
}
