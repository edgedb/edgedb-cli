use std::fs;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use crate::platform::config_dir;
use crate::repl;


#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct Config {
    #[serde(skip, default)]
    pub file_name: Option<PathBuf>,
    pub shell: ShellConfig,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct ShellConfig {
    #[serde(default)]
    pub expand_strings: Option<bool>,
    #[serde(default)]
    pub history_size: Option<usize>,
    #[serde(default)]
    pub implicit_properties: Option<bool>,
    #[serde(with="serde_str::opt", default)]
    pub input_mode: Option<repl::InputMode>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub idle_transaction_timeout: Option<usize>,
    #[serde(with="serde_str::opt", default)]
    pub output_format: Option<repl::OutputFormat>,
    #[serde(default)]
    pub display_typenames: Option<bool>,
    #[serde(with="serde_str::opt", default)]
    pub print_stats: Option<repl::PrintStats>,
    #[serde(default)]
    pub verbose_errors: Option<bool>,
}

pub fn get_config() -> anyhow::Result<Config> {
    let path = config_dir()?.join("cli.toml");
    if path.exists() {
        read_config(&path)
    } else {
        Ok(Default::default())
    }
}

#[context("reading file {:?}", path.as_ref())]
fn read_config(path: impl AsRef<Path>) -> anyhow::Result<Config> {
    let text = fs::read_to_string(&path)?;
    let mut toml = toml::de::Deserializer::new(&text);
    let mut val: Config = serde_path_to_error::deserialize(&mut toml)?;
    val.file_name = Some(path.as_ref().to_path_buf());
    Ok(val)
}
