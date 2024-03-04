use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fn_error_context::context;

use toml::Spanned;

use crate::platform::tmp_file_path;
use crate::portable::repository::{Channel, Query};
use crate::print::{echo, Highlight};

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SrcConfig {
    pub edgedb: SrcEdgedb,
    pub project: Option<SrcProject>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SrcEdgedb {
    #[serde(default)]
    pub server_version: Option<toml::Spanned<Query>>,
    #[serde(default)]
    pub branch: Option<Spanned<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
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
    pub branch: String,
}

#[derive(Debug)]
pub struct Project {
    pub schema_dir: PathBuf,
}

pub fn warn_extra(extra: &BTreeMap<String, toml::Value>, prefix: &str) {
    for key in extra.keys() {
        log::warn!("Unknown config option `{}{}`", prefix, key.escape_default());
    }
}

pub fn format_config(version: &Query) -> String {
    return format!(
        "\
        [edgedb]\n\
        server-version = {:?}\n\
    ",
        version.as_config_value()
    );
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
            server_version: val
                .edgedb
                .server_version
                .map(|x| x.into_inner())
                .unwrap_or(Query {
                    channel: Channel::Stable,
                    version: None,
                }),
            branch: val
                .edgedb
                .branch
                .map(|x| x.into_inner())
                .unwrap_or("main".to_string()),
        },
        project: Project {
            schema_dir: val
                .project
                .map(|p| p.schema_dir)
                .flatten()
                .map(|s| s.into())
                .unwrap_or_else(|| path.parent().unwrap_or(&Path::new("")).join("dbschema")),
        },
    });
}

pub fn modify_core<Selector, Value, Formatter, Config>(
    parsed: &Config,
    input: &String,
    config: &Path,
    selector: Selector,
    value: &Value,
    format: Formatter,
) -> anyhow::Result<bool>
where
    Selector: Fn(&Config) -> &Option<Spanned<Value>>,
    Value: std::cmp::PartialEq,
    Formatter: FnOnce(&Value) -> String,
{
    use std::fmt::Write;

    if let Some(selected) = selector(parsed) {
        if selected.get_ref() == value {
            return Ok(false);
        }

        let mut out = String::with_capacity(input.len() + 5);
        write!(
            &mut out,
            "{}{:?}{}",
            &input[..selected.start()],
            format(value),
            &input[selected.end()..]
        )
        .unwrap();

        let tmp = tmp_file_path(config);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, out)?;
        fs::rename(&tmp, config)?;

        return Ok(true);
    }

    println!("No selector");

    Ok(false)
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

#[context("cannot modify `{}`", config.display())]
pub fn modify_server_ver(config: &Path, ver: &Query) -> anyhow::Result<bool> {
    echo!(
        "Setting `server-version = ",
        format_args!("{:?}", ver.as_config_value()).emphasize(),
        "` in `edgedb.toml`"
    );
    modify(
        config,
        |v: &SrcConfig| &v.edgedb.server_version,
        ver,
        Query::as_config_value,
    )
}

#[context("cannot modify `{}`", config.display())]
pub fn modify_branch(config: &Path, branch: &String) -> anyhow::Result<bool> {
    echo!(
        "Setting `branch = ",
        format_args!("{:?}", branch).emphasize(),
        "` in `edgedb.toml`"
    );
    modify(
        config,
        |v: &SrcConfig| &v.edgedb.branch,
        branch,
        |v| v.clone(),
    )
}

#[cfg(test)]
mod test {
    use test_case::test_case;

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
    const TOML_2_3: &str = "\
        [edgedb]\n\
        server-version = \"2.3\"\n\
    ";
    const TOML_2_3_EXACT: &str = "\
        [edgedb]\n\
        server-version = \"=2.3\"\n\
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
    #[test_case(TOML_2_3, "=2.3" => Some(TOML_2_3_EXACT.into()))]
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
        super::modify_core(src, &ver.parse().unwrap()).unwrap()
    }
}
