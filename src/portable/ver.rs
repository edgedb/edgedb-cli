use std::fmt;
use std::str::FromStr;
use std::cmp::Ordering;

use anyhow::Context;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::connect::Connection;
use crate::process::{self, IntoArg};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Build(Box<str>);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Semver(semver::Version);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specific {
    pub major: u32,
    pub minor: MinorVersion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MinorVersion {
    Alpha(u32),
    Beta(u32),
    Rc(u32),
    Dev(u32),
    Minor(u32),
}

/// Version stored in config and in various `--version=` args
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub major: u32,
    pub minor: Option<FilterMinor>,
    pub exact: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub enum FilterMinor {
    Alpha(u32),
    Beta(u32),
    Rc(u32),
    Minor(u32),
}

static BUILD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\d+\.\d+(?:-(?:alpha|beta|rc|dev)\.\d+)?\+(?:[a-f0-9]{7}|local)$"#)
        .unwrap()
});

static SPECIFIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\d+)(?:\.0-(alpha|beta|rc|dev)\.(\d+)|\.(\d+))(?:$|\+)"#)
        .unwrap()
});

static FILTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?x)
        ^(?P<marker>=)?
        (?P<major>\d+)
        (?:
             \.0-(?P<dev>alpha|beta|rc)\.(?P<dev_num>\d+) |
             \.(?P<minor>\d+)
        )?$
    "#).unwrap()
});
static OLD_FILTER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?x)
        ^(?P<major>\d+)
        (?:
            (?:\.0)?-(?P<dev>alpha|beta|rc)\.?(?P<dev_num>\d+) |
            \.(?P<minor>\d+)
        )?$
    "#).unwrap()
});

impl FromStr for Build {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> anyhow::Result<Build> {
        if !BUILD.is_match(value) {
            anyhow::bail!("unsupported build version format: {}", value);
        }
        Ok(Build(value.into()))
    }
}

impl FromStr for Specific {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> anyhow::Result<Specific> {
        let m = SPECIFIC.captures(value)
            .context("unsupported version format. Examples: \
                     `1.15`, `7.0`, `3.0-rc.1`")?;
        let major = m.get(1).unwrap().as_str().parse()?;
        let g3 = m.get(3).map(|m| m.as_str().parse()).transpose()?;
        let minor = match m.get(2).map(|m| m.as_str()) {
            Some("alpha") => MinorVersion::Alpha(g3.unwrap()),
            Some("beta") => MinorVersion::Beta(g3.unwrap()),
            Some("rc") => MinorVersion::Rc(g3.unwrap()),
            Some("dev") => MinorVersion::Dev(g3.unwrap()),
            Some(_) => unreachable!(),
            None => MinorVersion::Minor(
                m.get(4).map(|m| m.as_str().parse()).transpose()?.unwrap()),
        };
        Ok(Specific { major, minor })
    }
}

impl FromStr for Filter {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> anyhow::Result<Filter> {
        let mut deprecated = false;
        let m = match FILTER.captures(value) {
            Some(m) => m,
            None => match OLD_FILTER.captures(value) {
                Some(m) => {
                    deprecated = true;
                    m
                }
                None => anyhow::bail!("unsupported version format. Examples: \
                     `1.15`, `7`, `3.0-rc.1`"),
            }
        };
        let major = m.name("major").unwrap().as_str().parse()?;
        let g3 = m.name("dev_num").map(|m| m.as_str().parse()).transpose()?;
        let minor = match m.name("dev").map(|m| m.as_str()) {
            Some("alpha") => g3.map(FilterMinor::Alpha),
            Some("beta") => g3.map(FilterMinor::Beta),
            Some("rc") => g3.map(FilterMinor::Rc),
            Some(_) => unreachable!(),
            None => m.name("minor").map(|m| m.as_str().parse()).transpose()?
                    .map(FilterMinor::Minor),
        };
        let exact = m.name("marker")
            .map(|m| m.as_str() == "=").unwrap_or(false)
            && minor.is_some();
        let result = Filter { major, minor, exact };
        if deprecated {
            log::warn!("Version numbers spelled as {:?} are deprecated. \
                        Use: {:?}.", value, result.to_string());
        }
        Ok(result)
    }
}

impl IntoArg for &Filter {
    fn add_arg(self, process: &mut process::Native) {
        process.arg(self.to_string());
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use FilterMinor::*;
        if self.exact {
            f.write_str("=")?;
        }
        match self.minor {
            None => write!(f, "{}", self.major),
            Some(Alpha(v)) => write!(f, "{}.0-alpha.{}", self.major, v),
            Some(Beta(v)) => write!(f, "{}.0-beta.{}", self.major, v),
            Some(Rc(v)) => write!(f, "{}.0-rc.{}", self.major, v),
            Some(Minor(v)) => write!(f, "{}.{}", self.major, v),
        }
    }
}

impl Build {
    pub fn is_nightly(&self) -> bool {
        self.0.contains("-dev.")
    }
    pub fn specific(&self) -> Specific {
        Specific::from_str(&self.0[..]).expect("build version is valid")
    }
    fn comparator(&self) -> Specific {
        self.specific()
    }
}

impl Specific {
    pub fn is_nightly(&self) -> bool {
        matches!(self.minor, MinorVersion::Dev(_))
    }

    pub fn is_testing(&self) -> bool {
        !(self.is_nightly() || self.is_stable())
    }

    pub fn is_stable(&self) -> bool {
        matches!(self.minor, MinorVersion::Minor(_))
    }
}

impl Filter {
    pub fn matches(&self, bld: &Build) -> bool {
        self.matches_specific(&bld.specific())
    }

    pub fn matches_specific(&self, spec: &Specific) -> bool {
        use MinorVersion as M;
        use FilterMinor as Q;

        if spec.major != self.major {
            return false;
        }

        if self.exact {
            match (spec.minor, self.minor.unwrap_or(Q::Minor(0))) {
                // dev releases can't be matched
                (M::Dev(_), _) => false,
                (M::Minor(v), Q::Minor(q)) => v == q,
                (M::Alpha(v), Q::Alpha(q)) => v == q,
                (M::Beta(v), Q::Beta(q)) => v == q,
                (M::Rc(v), Q::Rc(q)) => v == q,
                (_, _) => false,
            }
        } else {
            match (spec.minor, self.minor.unwrap_or(Q::Minor(0))) {
                // dev releases can't be matched
                (M::Dev(_), _) => false,
                // minor releases are upgradeable
                (M::Minor(v), Q::Minor(q)) => v >= q,
                // Special-case before 1.0, to treat all prereleases as major
                (M::Minor(_), _) if spec.major == 1 => false,
                (M::Alpha(v), Q::Alpha(q)) if spec.major == 1 => v == q,
                (M::Beta(v), Q::Beta(q)) if spec.major == 1 => v == q,
                (M::Rc(v), Q::Rc(q)) if spec.major == 1 => v == q,
                (_, _) if spec.major == 1 => false,
                // stable versions match prerelease pattern
                (M::Minor(_), _) => true,
                // prerelease versions match as >=
                (M::Alpha(v), Q::Alpha(q)) => v >= q,
                (M::Beta(_), Q::Alpha(_)) => true,
                (M::Rc(_), Q::Alpha(_)) => true,
                (M::Beta(v), Q::Beta(q))  => v >= q,
                (M::Rc(_), Q::Beta(_)) => true,
                (M::Rc(v), Q::Rc(q)) => v >= q,
                (_, _) => false,
            }
        }
    }
}

impl Specific {
    pub fn is_compatible(&self, other: &Specific) -> bool {
        use MinorVersion::*;
        match (&self.minor, &other.minor) {
            (Minor(_), Minor(_)) if self.major == other.major => true,
            // all dev/alpha/rc are incompatible as well as different major
            // but fully matching versions are always compatible
            _ => self == other,
        }
    }
}

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for Specific {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.major.fmt(f)?;
        f.write_str(".")?;
        match self.minor {
            MinorVersion::Minor(m) => m.fmt(f),
            MinorVersion::Alpha(v) => write!(f, "0-alpha.{}", v),
            MinorVersion::Beta(v) => write!(f, "0-beta.{}", v),
            MinorVersion::Rc(v) => write!(f, "0-rc.{}", v),
            MinorVersion::Dev(v) => write!(f, "0-dev.{}", v),
        }
    }
}

impl PartialEq for Build {
    fn eq(&self, other: &Build) -> bool {
        self.comparator() == other.comparator()
    }
}

impl Eq for Build {}

impl PartialOrd for Build {
    fn partial_cmp(&self, other: &Build) -> Option<Ordering> {
        self.comparator().partial_cmp(&other.comparator())
    }
}

impl Ord for Build {
    fn cmp(&self, other: &Build) -> Ordering {
        self.comparator().cmp(&other.comparator())
    }
}

impl fmt::Display for Semver {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Semver {
    type Err = semver::Error;
    fn from_str(s: &str) -> Result<Semver, semver::Error> {
        s.parse().map(Semver)
    }
}

impl PartialEq for Semver {
    fn eq(&self, other: &Semver) -> bool {
        let a = (&self.0.major, &self.0.minor, &self.0.patch, &self.0.pre);
        let b = (&other.0.major, &other.0.minor, &other.0.patch, &other.0.pre);
        a == b
    }
}

impl Eq for Semver {}

impl PartialOrd for Semver {
    fn partial_cmp(&self, other: &Semver) -> Option<Ordering> {
        let a = (&self.0.major, &self.0.minor, &self.0.patch, &self.0.pre);
        let b = (&other.0.major, &other.0.minor, &other.0.patch, &other.0.pre);
        a.partial_cmp(&b)
    }
}

impl Ord for Semver {
    fn cmp(&self, other: &Semver) -> Ordering {
        let a = (&self.0.major, &self.0.minor, &self.0.patch, &self.0.pre);
        let b = (&other.0.major, &other.0.minor, &other.0.patch, &other.0.pre);
        a.cmp(&b)
    }
}

pub async fn check_client(cli: &mut Connection, minimum_version: &Filter)
    -> anyhow::Result<bool>
{
    let ver = cli.get_version().await?;
    return Ok(ver.is_nightly() || minimum_version.matches(&ver));
}

#[test]
fn filter() {
    assert_eq!("2".parse::<Filter>().unwrap(), Filter {
        major: 2,
        minor: None,
        exact: false,
    });
    assert_eq!("2.3".parse::<Filter>().unwrap(), Filter {
        major: 2,
        minor: Some(FilterMinor::Minor(3)),
        exact: false,
    });
    assert_eq!("=2.3".parse::<Filter>().unwrap(), Filter {
        major: 2,
        minor: Some(FilterMinor::Minor(3)),
        exact: true,
    });
    assert_eq!("=2".parse::<Filter>().unwrap(), Filter {
        major: 2,
        minor: None,
        exact: false,
    });
}
