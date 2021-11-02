use std::fmt;
use std::str::FromStr;
use std::cmp::Ordering;

use anyhow::Context;
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Build(Box<str>);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specific {
    major: u32,
    minor: MinorVersion,
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
    major: u32,
    minor: Option<FilterMinor>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterMinor {
    Alpha(u32),
    Beta(u32),
    Rc(u32),
    Minor(u32),
}

static BUILD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\d+\.\d+(?:-(?:alpha|beta|rc|dev)\.\d+)?\+[a-f0-9]{7}$"#)
        .unwrap()
});

static SPECIFIC: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\d+)(?:\.0-(alpha|beta|rc|dev)\.(\d+)|\.(\d+))(?:$|\+)"#)
        .unwrap()
});

static QUERY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(\d+)(?:\.0-(alpha|beta|rc)\.(\d+)|\.(\d+))?$"#)
        .unwrap()
});

// TODO(tailhook) remove me after json is fixed
fn fixup_build(value: &str) -> Option<Build> {
    static MINOR: Lazy<Regex> = Lazy::new(
        || Regex::new(r"^\d+\.\d+").unwrap());
    static DEV: Lazy<Regex> = Lazy::new(
        || Regex::new(r"\.dev\d+").unwrap());
    static HASH: Lazy<Regex> = Lazy::new(
        || Regex::new(r"\.g[a-f0-9]{7}").unwrap());

    let minor = MINOR.find(value)?.as_str();
    let dev = &DEV.find(value)?.as_str()[4..];
    let hash = &HASH.find(value)?.as_str()[2..];
    Some(format!("{}-dev.{}+{}", minor, dev, hash).parse().unwrap())
}

impl FromStr for Build {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> anyhow::Result<Build> {
        if !BUILD.is_match(value) {
            return fixup_build(value)
                .context("unsupported build version format").into();
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
        let m = QUERY.captures(value)
            .context("unsupported version format. Examples: \
                     `1.15`, `7`, `3.0-rc.1`")?;
        let major = m.get(1).unwrap().as_str().parse()?;
        let g3 = m.get(3).map(|m| m.as_str().parse()).transpose()?;
        let minor = match m.get(2).map(|m| m.as_str()) {
            Some("alpha") => g3.map(FilterMinor::Alpha),
            Some("beta") => g3.map(FilterMinor::Beta),
            Some("rc") => g3.map(FilterMinor::Rc),
            Some(_) => unreachable!(),
            None => m.get(4).map(|m| m.as_str().parse()).transpose()?
                    .map(FilterMinor::Minor),
        };
        Ok(Filter { major, minor })
    }
}

impl Build {
    pub fn specific(&self) -> Specific {
        Specific::from_str(&self.0[..]).expect("build version is valid")
    }
    fn comparator(&self) -> crate::server::version::Version<&str> {
        let end = self.0.as_bytes().iter().position(|&c| c == b'+')
            .unwrap_or(self.0.len());
        crate::server::version::Version(&self.0[..end])
    }
}

impl Filter {
    pub fn matches(&self, bld: &Build) -> bool {
        use MinorVersion as M;
        use FilterMinor as Q;

        let spec = bld.specific();
        if spec.major != self.major {
            return false;
        }

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
