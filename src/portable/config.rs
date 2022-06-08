use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use crate::commands::ExitCode;
use crate::portable::exit_codes;
use crate::portable::repository::{Channel, Query};
use crate::platform::tmp_file_path;
use crate::print::{self, echo, Highlight};


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
    pub server_version: Option<toml::Spanned<Query>>,
    #[serde(default)]
    pub schema_dir: Option<toml::Spanned<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug)]
pub struct Config {
    pub edgedb: Edgedb,
}

#[derive(Debug)]
pub struct Edgedb {
    pub server_version: Query,
    pub schema_dir: PathBuf
}

fn warn_extra(extra: &BTreeMap<String, toml::Value>, prefix: &str) {
    for key in extra.keys() {
        log::warn!("Unknown config option `{}{}`",
                   prefix, key.escape_default());
    }
}

pub fn format_config(version: &Query, schema_dir: &Path) -> String {
    return format!("\
        [edgedb]\n\
        server-version = {:?}\n\
        schema-dir = {:?}\n\
    ", version.as_config_value(), schema_dir)
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
            server_version: val.edgedb.server_version
                .map(|x| x.into_inner())
                .unwrap_or(Query {
                    channel: Channel::Stable,
                    version: None,
                }),
            schema_dir: val.edgedb.schema_dir
                .map(|x| x.into_inner())
                .unwrap_or("dbschema".into())
                .into()
        }
    })
}

fn toml_modify_config(data: &str, version: &Query, schema_dir: &Path)
    -> anyhow::Result<Option<String>>
{
    use std::fmt::Write;

    let mut toml = toml::de::Deserializer::new(&data);
    let parsed: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;
    if let Some(ver_position) = &parsed.edgedb.server_version {
        if let Some(schema_dir_position) = &parsed.edgedb.schema_dir {
            if ver_position.get_ref() == version {
                return Ok(None);
            }

            let schema_dir_length = schema_dir.display().to_string().len();
            let mut out = String::with_capacity(data.len() + 5 + schema_dir_length);
            write!(&mut out, "{}{:?}{}{:?}{}",
                &data[..ver_position.start()],
                version.as_config_value(),
                &data[ver_position.end()..schema_dir_position.start()],
                schema_dir,
                &data[schema_dir_position.end()..],
            ).unwrap();

            return Ok(Some(out));
        } else {
            print::error("No schema-dir found in `edgedb.toml`.");
        }
    } else {
        print::error("No server-version found in `edgedb.toml`.");
    }

    eprintln!("Please ensure that `edgedb.toml` contains:");
    println!("  {}",
        format_config(version, schema_dir)
        .lines()
        .collect::<Vec<_>>()
        .join("\n  "));
    return Err(ExitCode::new(exit_codes::INVALID_CONFIG).into());
}

#[context("cannot modify `{}`", config.display())]
pub fn modify(config: &Path, ver: &Query, schema_dir: &Path) -> anyhow::Result<bool> {
    let input = fs::read_to_string(&config)?;
    if let Some(output) = toml_modify_config(&input, ver, schema_dir)? {
        echo!("Setting `server-version = ",
               format_args!("{:?}", ver.as_config_value()).emphasize(),
               "and `schema-direcorty = ",
               format_args!("{:?}", schema_dir).emphasize(),
               " in `edgedb.toml`");
        let tmp = tmp_file_path(config);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, output)?;
        fs::rename(&tmp, config)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;
    use test_case::test_case;
    use super::toml_modify_config;

    const TOML_BETA1: &str = "\
        [edgedb]\n\
        server-version = \"1.0-beta.1\"\n\
        schema-dir = \"dbschema\"\n\
    ";
    const TOML_BETA2: &str = "\
        [edgedb]\n\
        server-version = \"1.0-beta.2\"\n\
        schema-dir = \"dbschema\"\n\
    ";
    const TOML_NIGHTLY: &str = "\
        [edgedb]\n\
        server-version = \"nightly\"\n\
        schema-dir = \"dbschema\"\n\
    ";
    const TOML_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        server-version = \"nightly\"\n\
        schema-dir = \"custom-dir\"\n\
    ";

    const TOML2_BETA1: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1.0-beta.1\" #and here\n\
        schema-dir = \"dbschema\"\n\
        other-setting = true\n\
    ";
    const TOML2_BETA2: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1.0-beta.2\" #and here\n\
        schema-dir = \"dbschema\"\n\
        other-setting = true\n\
    ";
    const TOML2_NIGHTLY: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"nightly\" #and here\n\
        schema-dir = \"dbschema\"\n\
        other-setting = true\n\
    ";
    const TOML2_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"nightly\" #and here\n\
        schema-dir = \"custom-dir\"\n\
        other-setting = true\n\
    ";

    const TOMLI_BETA1: &str = "\
        edgedb = {server-version = \"1.0-beta.1\", schema-dir = \"dbschema\"}\n\
    ";
    const TOMLI_BETA2: &str = "\
        edgedb = {server-version = \"1.0-beta.2\", schema-dir = \"dbschema\"}\n\
    ";
    const TOMLI_NIGHTLY: &str = "\
        edgedb = {server-version = \"nightly\", schema-dir = \"dbschema\"}\n\
    ";
    const TOMLI_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        edgedb = {server-version = \"nightly\", schema-dir = \"custom-dir\"}\n\
    ";

    #[test_case(TOML_BETA1, "1.0-beta.2", "dbschema" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA2, "1.0-beta.2", "dbschema" => None)]
    #[test_case(TOML_NIGHTLY, "1.0-beta.2", "dbschema" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA1, "1.0-beta.1", "dbschema" => None)]
    #[test_case(TOML_BETA2, "1.0-beta.1", "dbschema" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_NIGHTLY, "1.0-beta.1", "dbschema" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_BETA1, "nightly", "dbschema" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_BETA2, "nightly", "dbschema" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_BETA2, "nightly", "custom-dir" => Some(TOML_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOML_NIGHTLY, "nightly", "dbschema" => None)]

    #[test_case(TOML2_BETA1, "1.0-beta.2", "dbschema" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA2, "1.0-beta.2", "dbschema" => None)]
    #[test_case(TOML2_NIGHTLY, "1.0-beta.2", "dbschema" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA1, "1.0-beta.1", "dbschema" => None)]
    #[test_case(TOML2_BETA2, "1.0-beta.1", "dbschema" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_NIGHTLY, "1.0-beta.1", "dbschema" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_BETA1, "nightly", "dbschema" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_BETA2, "nightly", "dbschema" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_BETA2, "nightly", "custom-dir" => Some(TOML2_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOML2_NIGHTLY, "nightly", "dbschema" => None)]

    #[test_case(TOMLI_BETA1, "1.0-beta.2", "dbschema" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA2, "1.0-beta.2", "dbschema" => None)]
    #[test_case(TOMLI_NIGHTLY, "1.0-beta.2", "dbschema" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA1, "1.0-beta.1", "dbschema" => None)]
    #[test_case(TOMLI_BETA2, "1.0-beta.1", "dbschema" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_NIGHTLY, "1.0-beta.1", "dbschema" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_BETA1, "nightly", "dbschema" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_BETA2, "nightly", "dbschema" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_BETA2, "nightly", "custom-dir" => Some(TOMLI_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOMLI_NIGHTLY, "nightly", "dbschema" => None)]
    fn modify(src: &str, ver: &str, schema_dir: &str) -> Option<String> {
        toml_modify_config(src, &ver.parse().unwrap(), Path::new(schema_dir)).unwrap()
    }

}
