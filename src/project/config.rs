use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use fn_error_context::context;

use crate::server::version::VersionQuery;


#[derive(serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct SrcConfig {
    pub edgedb: SrcEdgedb,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct SrcEdgedb {
    #[serde(default)]
    pub server_version: Option<toml::Spanned<VersionQuery>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug)]
pub struct Config {
    pub edgedb: Edgedb,
}

#[derive(Debug)]
pub struct Edgedb {
    pub server_version: Option<VersionQuery>,
}

fn warn_extra(extra: &BTreeMap<String, toml::Value>, prefix: &str) {
    for key in extra.keys() {
        log::warn!("Unknown config option `{}{}`",
                   prefix, key.escape_default());
    }
}

#[context("error reading project config `{}`", path.display())]
pub fn read(path: &Path) -> anyhow::Result<Config> {
    let text = fs::read_to_string(&path)?;
    let mut toml = toml::de::Deserializer::new(&text);
    let val: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;
    warn_extra(&val.extra, "");
    warn_extra(&val.edgedb.extra, "edgedb.");
    return Ok(Config {
        edgedb: Edgedb {
            server_version: val.edgedb.server_version.map(|x| x.into_inner()),
        }
    })
}
