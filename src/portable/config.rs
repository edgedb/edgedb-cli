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
    pub project: Option<SrcProject>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct SrcEdgedb {
    #[serde(default)]
    pub server_version: Option<toml::Spanned<Query>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all="kebab-case")]
pub struct SrcProject {
    #[serde(default)]
    pub schema_dir: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}


#[derive(Debug)]
pub struct Config {
    pub edgedb: Edgedb,
    pub project: Project,
}

#[derive(Debug)]
pub struct Edgedb {
    pub server_version: Query,
}

#[derive(Debug)]
pub struct Project {
    pub schema_dir: PathBuf
}

fn warn_extra(extra: &BTreeMap<String, toml::Value>, prefix: &str) {
    for key in extra.keys() {
        log::warn!("Unknown config option `{}{}`",
                   prefix, key.escape_default());
    }
}

pub fn format_config(version: &Query) -> String {
    return format!("\
            [edgedb]\n\
            server-version = {:?}\n\
        ", version.as_config_value());
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
        },
        project: Project{
            schema_dir: val.project
                .map(|p| p.schema_dir)
                .flatten()
                .map(|s| s.into())
                .unwrap_or("dbschema".into())
        },
    })
}

fn toml_set_version(data: &str, version: &Query)
    -> anyhow::Result<Option<String>>
{
    use std::fmt::Write;

    let mut toml = toml::de::Deserializer::new(&data);
    let parsed: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;
    if let Some(ver_position) = &parsed.edgedb.server_version {
        if ver_position.get_ref() == version {
            return Ok(None);
        }
        let mut out = String::with_capacity(data.len() + 5);
        write!(&mut out, "{}{:?}{}",
            &data[..ver_position.start()],
            version.as_config_value(),
            &data[ver_position.end()..],
        ).unwrap();
        return Ok(Some(out));
    }
    print::error("No server-version found in `edgedb.toml`.");
    eprintln!("Please ensure that `edgedb.toml` contains:");
    println!("  {}",
        format_config(version)
        .lines()
        .collect::<Vec<_>>()
        .join("\n  "));
    return Err(ExitCode::new(exit_codes::INVALID_CONFIG).into());
}

#[context("cannot modify `{}`", config.display())]
pub fn modify(config: &Path, ver: &Query) -> anyhow::Result<bool> {
    let input = fs::read_to_string(&config)?;
    if let Some(output) = toml_set_version(&input, ver)? {
        echo!("Setting `server-version = ",
               format_args!("{:?}", ver.as_config_value()).emphasize(),
               "` in `edgedb.toml`");
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
    use test_case::test_case;
    use super::toml_set_version;

    const TOML_BETA1: &str = "\
        [edgedb]\n\
        server-version = \"1.0-beta.1\"\n\
    ";
    const TOML_BETA2: &str = "\
        [edgedb]\n\
        server-version = \"1.0-beta.2\"\n\
    ";
    const TOML_BETA2_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        server-version = \"1.0-beta.2\"\n\
        [project]
        schema-dir = \"custom-dir\"\n\
    ";
    const TOML_NIGHTLY: &str = "\
        [edgedb]\n\
        server-version = \"nightly\"\n\
    ";
    const TOML_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        server-version = \"nightly\"\n\
        [project]
        schema-dir = \"custom-dir\"\n\
    ";

    const TOML2_BETA1: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1.0-beta.1\" #and here\n\
        other-setting = true\n\
    ";
    const TOML2_BETA2: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1.0-beta.2\" #and here\n\
        other-setting = true\n\
    ";
    const TOML2_BETA2_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1.0-beta.2\" #and here\n\
        other-setting = true\n\
        [project]
        schema-dir = \"custom-dir\"\n\
    ";
    const TOML2_NIGHTLY: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"nightly\" #and here\n\
        other-setting = true\n\
    ";
    const TOML2_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"nightly\" #and here\n\
        other-setting = true\n\
        [project]
        schema-dir = \"custom-dir\"\n\
    ";

    const TOMLI_BETA1: &str = "\
        edgedb = {server-version = \"1.0-beta.1\"}\n\
    ";
    const TOMLI_BETA2: &str = "\
        edgedb = {server-version = \"1.0-beta.2\"}\n\
    ";
    const TOMLI_BETA2_CUSTOM_SCHEMA_DIR: &str = "\
        edgedb = {server-version = \"1.0-beta.2\"}\n\
        project = {schema-dir = \"custom-dir\"}\n\
    ";
    const TOMLI_NIGHTLY: &str = "\
        edgedb = {server-version = \"nightly\"}\n\
    ";
    const TOMLI_NIGHTLY_CUSTOM_SCHEMA_DIR: &str = "\
        edgedb = {server-version = \"nightly\"}\n\
        project = {schema-dir = \"custom-dir\"}\n\
    ";
    #[test_case(TOML_BETA1, "1.0-beta.2" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA2, "1.0-beta.2" => None)]
    #[test_case(TOML_NIGHTLY, "1.0-beta.2" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA1, "1.0-beta.1" => None)]
    #[test_case(TOML_BETA2, "1.0-beta.1" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_NIGHTLY, "1.0-beta.1" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_BETA1, "nightly" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_BETA2, "nightly" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_BETA2_CUSTOM_SCHEMA_DIR, "nightly" => Some(TOML_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOML_NIGHTLY, "nightly" => None)]

    #[test_case(TOML2_BETA1, "1.0-beta.2" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA2, "1.0-beta.2" => None)]
    #[test_case(TOML2_NIGHTLY, "1.0-beta.2" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA1, "1.0-beta.1" => None)]
    #[test_case(TOML2_BETA2, "1.0-beta.1" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_NIGHTLY, "1.0-beta.1" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_BETA1, "nightly" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_BETA2, "nightly" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_BETA2_CUSTOM_SCHEMA_DIR, "nightly" => Some(TOML2_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOML2_NIGHTLY, "nightly" => None)]

    #[test_case(TOMLI_BETA1, "1.0-beta.2" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA2, "1.0-beta.2" => None)]
    #[test_case(TOMLI_NIGHTLY, "1.0-beta.2" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA1, "1.0-beta.1" => None)]
    #[test_case(TOMLI_BETA2, "1.0-beta.1" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_NIGHTLY, "1.0-beta.1" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_BETA1, "nightly" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_BETA2, "nightly" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_BETA2_CUSTOM_SCHEMA_DIR, "nightly"=> Some(TOMLI_NIGHTLY_CUSTOM_SCHEMA_DIR.into()))]
    #[test_case(TOMLI_NIGHTLY, "nightly" => None)]
    fn modify(src: &str, ver: &str) -> Option<String> {
        toml_set_version(src, &ver.parse().unwrap()).unwrap()
    }

}
