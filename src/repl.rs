use async_std::sync::{Sender, Receiver};

use crate::prompt;
use crate::print;

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

pub struct State {
    pub control: Sender<prompt::Control>,
    pub data: Receiver<prompt::Input>,
    pub print: print::Config,
    pub verbose_errors: bool,
    pub last_error: Option<anyhow::Error>,
    pub database: String,
    pub implicit_limit: Option<usize>,
    pub input_mode: InputMode,
    pub output_mode: OutputMode,
}


impl State {
    pub async fn edgeql_input(&mut self, initial: &str) -> prompt::Input {
        self.control.send(
            prompt::Control::EdgeqlInput {
                database: self.database.clone(),
                initial: initial.to_owned(),
            }
        ).await;
        match self.data.recv().await {
            None | Some(prompt::Input::Eof) => prompt::Input::Eof,
            Some(x) => x,
        }
    }
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
            None | Some(prompt::Input::Eof) => prompt::Input::Eof,
            Some(x) => x,
        }
    }
    pub async fn input_mode(&self, value: InputMode) {
        let msg = match value {
            InputMode::Vi => prompt::Control::ViMode,
            InputMode::Emacs => prompt::Control::EmacsMode,
        };
        self.control.send(msg).await
    }
    pub async fn show_history(&self) {
        self.control.send(prompt::Control::ShowHistory).await;
    }
    pub async fn spawn_editor(&self, entry: Option<isize>) -> prompt::Input {
        self.control.send(prompt::Control::SpawnEditor { entry }).await;
        match self.data.recv().await {
            None | Some(prompt::Input::Eof) => prompt::Input::Eof,
            Some(x) => x,
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
