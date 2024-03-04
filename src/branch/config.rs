use crate::platform::tmp_file_path;
use crate::portable::config::{modify_core, warn_extra};

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use toml::Spanned;

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SrcConfig {
    #[serde(default)]
    pub current_branch: Option<Spanned<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

pub struct Config {
    pub current_branch: String,
}

pub fn create_or_read(path: &Path, default_branch: Option<&str>) -> anyhow::Result<Config> {
    if path.exists() {
        return read(path);
    }

    let branch = default_branch.unwrap_or("main");
    let tmp = tmp_file_path(path);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, format!("current-branch = \"{}\"", branch))?;
    fs::rename(&tmp, path)?;

    Ok(Config {
        current_branch: branch.to_string(),
    })
}

pub fn read(path: &Path) -> anyhow::Result<Config> {
    let text = fs::read_to_string(&path)?;
    let mut toml = toml::de::Deserializer::new(&text);
    let val: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;

    warn_extra(&val.extra, "");

    Ok(Config {
        current_branch: val
            .current_branch
            .map(|x| x.into_inner())
            .unwrap_or("main".to_string()),
    })
}

fn modify<T, U, V>(config: &Path, selector: T, value: &U, format: V) -> anyhow::Result<bool>
where
    T: Fn(&SrcConfig) -> &Option<Spanned<U>>,
    U: std::cmp::PartialEq,
    V: FnOnce(&U) -> String,
{
    let input = fs::read_to_string(&config)?;
    let mut toml = toml::de::Deserializer::new(&input);
    let parsed: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;

    return modify_core(&parsed, &input, config, selector, value, format);
}

pub fn modify_current_branch(config: &Path, branch: &String) -> anyhow::Result<bool> {
    modify(
        config,
        |v: &SrcConfig| &v.current_branch,
        branch,
        |v| v.clone(),
    )
}
