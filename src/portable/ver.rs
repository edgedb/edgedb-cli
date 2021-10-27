use std::str::FromStr;

use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub struct Build(Box<str>);

#[derive(Clone, Debug, PartialEq)]
pub struct Ver<'a>(&'a str);

static REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\d+\.\d+(?:-(?:alpha|beta|rc|dev)\.\d+)?\+[a-f0-9]{7}$"#)
        .unwrap()
});

impl FromStr for Build {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> anyhow::Result<Build> {
        //TODO(tailhook) validate by regex
        Ok(Build(value.into()))
    }
}
