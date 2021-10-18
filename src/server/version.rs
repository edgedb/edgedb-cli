use std::fmt;
use std::cmp::Ordering;
use std::iter::{Peekable};
use std::str::CharIndices;

use serde::{ser, de, Deserialize, Serialize};

use crate::server::detect::InstalledPackage;
use crate::server::distribution::DistributionRef;


/// Used in metadata to store used version
#[derive(Debug, Clone, PartialEq)]
pub enum VersionMarker {
    Stable(Version<String>),
    Nightly,
}

/// Used in to distinguish between versions by slot name
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionSlot {
    Stable(Version<String>),
    Nightly(Version<String>),
}

/// Used to search for a version by CLI params or edgedb.coml
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionQuery {
    Stable(Option<Version<String>>),
    Nightly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version<T: AsRef<str>>(pub T);
pub struct Components<'a>(&'a str, Peekable<CharIndices<'a>>);

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Component<'a> {
    Numeric(u64),
    String(&'a str),
}

impl<T: AsRef<str>> ::std::fmt::Display for Version<T> {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        self.0.as_ref().fmt(fmt)
    }
}

impl<'a> ::std::fmt::Display for Component<'a> {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        match *self {
            Component::Numeric(v) => v.fmt(fmt),
            Component::String(v) => v.fmt(fmt),
        }
    }
}

impl<T: AsRef<str>> AsRef<str> for Version<T> {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<T: AsRef<str>> Version<T> {
    pub fn to_ref(&self) -> Version<&str> {
        Version(self.0.as_ref())
    }
}

impl<T: AsRef<str>> Version<T> {
    pub fn num(&self) -> &str {
        let s = self.0.as_ref();
        if s.starts_with("v") {
            &s[1..]
        } else {
            s
        }
    }
    pub fn components(&self) -> Components {
        let mut ch = self.0.as_ref().char_indices().peekable();
        if ch.peek() == Some(&(0, 'v')) {
            ch.next();
        }
        return Components(self.0.as_ref(), ch);
    }
}

impl std::str::FromStr for Version<String> {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Version(s.into()))
    }
}


impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;
    fn next(&mut self) -> Option<Component<'a>> {
        use self::Component::*;
        while let Some(&(_, x)) = self.1.peek() {
            if x == '+' {
                // Ignore anything after +, i.e. treat it as end of string
                for _ in self.1.by_ref() {}
                return None;
            }
            if x.is_alphanumeric() { break; }
            self.1.next();
        }
        if let Some(&(start, x)) = self.1.peek() {
            if x.is_numeric() {
                while let Some(&(_, x)) = self.1.peek() {
                    if !x.is_numeric() { break; }
                    self.1.next();
                }
                let end = self.1.peek().map(|&(x, _)| x)
                    .unwrap_or(self.0.len());
                let val = &self.0[start..end];
                return Some(val.parse().map(Numeric).unwrap_or(String(val)));
            } else {
                while let Some(&(_, x)) = self.1.peek() {
                    if !x.is_alphanumeric() { break; }
                    self.1.next();
                }
                let end = self.1.peek().map(|&(x, _)| x)
                    .unwrap_or(self.0.len());
                let val = &self.0[start..end];
                return Some(String(val));
            }
        }
        None
    }
}

impl<A: AsRef<str>, B: AsRef<str>> PartialEq<Version<B>> for Version<A> {
    fn eq(&self, other: &Version<B>) -> bool {
        raw_cmp(self, other) == Ordering::Equal
    }
}

impl<T: AsRef<str>> Eq for Version<T> {}

impl<A: AsRef<str>, B: AsRef<str>> PartialOrd<Version<B>> for Version<A> {
    fn partial_cmp(&self, other: &Version<B>) -> Option<Ordering> {
        Some(raw_cmp(self, other))
    }
}

impl<T: AsRef<str>> Ord for Version<T> {
    fn cmp(&self, other: &Version<T>) -> Ordering {
        raw_cmp(self, other)
    }
}

fn raw_cmp(this: &Version<impl AsRef<str>>, other: &Version<impl AsRef<str>>)
    -> Ordering
{
    use self::Component::*;
    use std::cmp::Ordering::*;
    let mut aiter = this.components();
    let mut biter = other.components();
    loop {
        let val = match (aiter.next(), biter.next()) {
            (Some(Numeric(x)), Some(Numeric(y))) => x.cmp(&y),
            (Some(Numeric(_)), Some(String(_))) => Greater,
            (Some(String(_)), Some(Numeric(_))) => Less,
            (Some(String(x)), Some(String(y))) => x.cmp(y),
            (Some(Numeric(_)), None) => Greater,
            (None, Some(Numeric(_))) => Less,
            (None, Some(String(x)))
            if matches!(x, "a"|"b"|"c"|"rc"|"pre"|"dev"|"dirty")
            || x.starts_with("beta") || x.starts_with("rc")
            => Greater,
            (None, Some(String(_))) => Less,
            (Some(String(x)), None)
            // git revision starts with g
            if matches!(x, "a"|"b"|"c"|"rc"|"pre"|"dev"|"dirty")
            || x.starts_with("g")
            || x.starts_with("beta") || x.starts_with("rc")
            => Less,
            (Some(String(_)), None) => Greater,
            (None, None) => return Equal,
        };
        if val != Equal {
            return val;
        }
    }
}

impl VersionSlot {
    pub fn slot_name(&self) -> &Version<String> {
        match self {
            VersionSlot::Stable(ver) => ver,
            VersionSlot::Nightly(ver) => ver,
        }
    }
    pub fn to_query(&self) -> VersionQuery {
        match self {
            VersionSlot::Stable(ver) => VersionQuery::Stable(Some(ver.clone())),
            VersionSlot::Nightly(_) => VersionQuery::Nightly,
        }
    }
    pub fn to_marker(&self) -> VersionMarker {
        match self {
            VersionSlot::Stable(ver) => VersionMarker::Stable(ver.clone()),
            VersionSlot::Nightly(_) => VersionMarker::Nightly,
        }
    }
    pub fn title(&self) -> impl fmt::Display + '_ {
        match self {
            VersionSlot::Stable(ver) => ver.to_string(),
            VersionSlot::Nightly(slot) => format!("{} (nightly)", slot),
        }
    }
    pub fn is_nightly(&self) -> bool {
        matches!(self, VersionSlot::Nightly(_))
    }
    pub fn as_stable(&self) -> Option<&Version<String>> {
        match self {
            VersionSlot::Stable(v) => Some(v),
            VersionSlot::Nightly(_) => None,
        }
    }
}

impl VersionQuery {
    pub fn new(nightly: bool, version: Option<&Version<String>>)
        -> VersionQuery
    {
        if nightly {
            VersionQuery::Nightly
        } else {
            VersionQuery::Stable(version.cloned())
        }
    }
    pub fn is_nightly(&self) -> bool {
        matches!(self, VersionQuery::Nightly)
    }
    pub fn is_specific(&self) -> bool {
        matches!(self, VersionQuery::Stable(Some(..)))
    }
    pub fn to_arg(&self) -> Option<String> {
        use VersionQuery::*;

        match self {
            Stable(None) => None,
            Stable(Some(ver)) => Some(format!("--version={}", ver)),
            Nightly => Some("--nightly".into()),
        }
    }
    pub fn installed_matches(&self, pkg: &InstalledPackage) -> bool {
        use VersionQuery::*;

        match self {
            Nightly => pkg.is_nightly(),
            Stable(None) => !pkg.is_nightly(),
            Stable(Some(v)) => &pkg.major_version == v && !pkg.is_nightly(),
        }
    }
    pub fn matches(&self, version: &VersionSlot) -> bool {
        use VersionQuery as Q;
        use VersionSlot as S;

        match (self, version) {
            (Q::Nightly, S::Nightly(_)) => true,
            (Q::Stable(None), S::Stable(_)) => true,
            (Q::Stable(Some(q)), S::Stable(v)) if q == v => true,
            _ => false,
        }
    }
    pub fn marker_matches(&self, version: &VersionMarker) -> bool {
        use VersionQuery as Q;
        use VersionMarker as M;

        match (self, version) {
            (Q::Nightly, M::Nightly) => true,
            (Q::Stable(None), M::Stable(_)) => true,
            (Q::Stable(Some(q)), M::Stable(v)) if q == v => true,
            _ => false,
        }
    }
    pub fn distribution_matches(&self, distr: &DistributionRef) -> bool {
        self.matches(distr.version_slot())
    }
    pub fn install_option(&self) -> String {
        match self {
            VersionQuery::Nightly => "--nightly".into(),
            VersionQuery::Stable(None) => "".into(),
            VersionQuery::Stable(Some(ver)) => format!("--version={}", ver),
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            VersionQuery::Stable(Some(ver)) => ver.as_ref(),
            VersionQuery::Stable(None) => "*",
            VersionQuery::Nightly => "nightly",
        }
    }
}

impl fmt::Display for VersionQuery {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use VersionQuery::*;
        match self {
            Stable(None) => "stable".fmt(f),
            Stable(Some(ver)) => ver.fmt(f),
            Nightly => "nightly".fmt(f),
        }
    }
}

impl VersionMarker {
    pub fn is_nightly(&self) -> bool {
        matches!(self, VersionMarker::Nightly)
    }
    pub fn title(&self) -> &str {
        match self {
            VersionMarker::Stable(v) => v.num(),
            VersionMarker::Nightly => "nightly",
        }
    }
    pub fn to_query(&self) -> VersionQuery {
        match self {
            VersionMarker::Stable(v) => VersionQuery::Stable(Some(v.clone())),
            VersionMarker::Nightly => VersionQuery::Nightly,
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            VersionMarker::Stable(v) => v.num(),
            VersionMarker::Nightly => "nightly",
        }
    }
    pub fn as_stable(&self) -> Option<&Version<String>> {
        match self {
            VersionMarker::Stable(v) => Some(v),
            VersionMarker::Nightly => None,
        }
    }
}

impl<'de> Deserialize<'de> for VersionMarker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        match &s[..] {
            "nightly" => Ok(VersionMarker::Nightly),
            s => Ok(VersionMarker::Stable(Version(s.into()))),
        }
    }
}

impl<'de> Deserialize<'de> for VersionQuery {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        match &s[..] {
            "nightly" => Ok(VersionQuery::Nightly),
            "*" => Ok(VersionQuery::Stable(None)),
            s => Ok(VersionQuery::Stable(Some(Version(s.into())))),
        }
    }
}

impl Serialize for VersionMarker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        serializer.serialize_str(match self {
            VersionMarker::Stable(ver) => ver.num(),
            VersionMarker::Nightly => "nightly",
        })
    }
}

#[test]
fn test_version_marker_cmp() {
    assert!(
        // nightlies are always larger
        VersionSlot::Nightly(Version("1-beta1".into())) >
        VersionSlot::Stable(Version("1-beta2".into()))
    );
    assert!(
        VersionSlot::Stable(Version("1-beta3".into())) >
        VersionSlot::Stable(Version("1-beta2".into()))
    );
    assert!(
        VersionSlot::Stable(Version("1-rc".into())) >
        VersionSlot::Stable(Version("1-beta2".into()))
    );
    assert!(
        VersionSlot::Stable(Version("1-rc2".into())) >
        VersionSlot::Stable(Version("1-beta2".into()))
    );
    assert!(
        !(
            VersionSlot::Stable(Version("1-beta2".into())) >
            VersionSlot::Stable(Version("1-beta2".into()))
        )
    );
    assert!(
        VersionSlot::Stable(Version("1".into())) >
        VersionSlot::Stable(Version("1-beta2".into()))
    );
}


#[cfg(test)]
mod test {
    use super::Version;

    #[test]
    fn test_version_parse() {
        use super::Component::*;
        assert_eq!(Version("v0.4.1-28-gfba00d7")
            .components().collect::<Vec<_>>(),
            [Numeric(0), Numeric(4), Numeric(1),
             Numeric(28), String("gfba00d7")]);
        assert_eq!(Version("v0.4.1+trusty1")
            .components().collect::<Vec<_>>(),
            [Numeric(0), Numeric(4), Numeric(1)]);
    }

    #[test]
    fn test_version_cmp() {
        assert!(Version("v0.4.1-28-gfba00d7") > Version("v0.4.1"));
        assert!(Version("v0.4.1-28-gfba00d7") > Version("v0.4.1-27-gtest"));
        assert!(Version("v0.4.1-28-gfba00d7") < Version("v0.4.2"));
        assert!(Version("v0.4.1+trusty1") == Version("v0.4.1"));
        assert!(Version("v0.4.1+trusty1") == Version("v0.4.1+precise1"));
    }

    #[test]
    fn test_version_cmp2() {
        assert!(!(Version("v0.4.1-172-ge011471")
                < Version("v0.4.1-172-ge011471")));
    }

    #[test]
    fn semver_test() {
        // example from semver.org
        assert!(Version("1.0.0-alpha") < Version("1.0.0-alpha.1"));
        // This one is intentionally doesn't work, beta is always lower
        // than digits (we use PEP 440) from python as it seems more reasonable
        // for parsing arbitrary version number rather than fairly strict
        // semver
        // assert!(Version("1.0.0-alpha.1") < Version("1.0.0-alpha.beta"));
        assert!(Version("1.0.0-alpha.beta") < Version("1.0.0-beta"));
        assert!(Version("1.0.0-beta") < Version("1.0.0-beta.2"));
        assert!(Version("1.0.0-beta.2") < Version("1.0.0-beta.11"));
        assert!(Version("1.0.0-beta.11") < Version("1.0.0-rc.1"));
        assert!(Version("1.0.0-rc.1") < Version("1.0.0"));
    }

    #[test]
    fn edgedb_test() {
        assert!(Version("1-alpha2") < Version("1-alpha3"));
        assert!(!(Version("1-beta2") < Version("1-beta2")));
        assert!(Version("1-beta2") < Version("1-rc1"));
        assert!(Version("1-beta2") < Version("1"));
        assert!(Version("1-rc1") < Version("1"));
        assert!(Version("1-rc2") < Version("1"));
        assert!(Version("1") > Version("1-beta2"));
        assert!(Version("1") > Version("1-rc1"));
        assert!(Version("1") > Version("1-rc2"));
        assert!(Version("3") < Version("12"));
    }
}
