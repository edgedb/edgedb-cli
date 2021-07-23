use std::str::CharIndices;
use std::iter::{Peekable};
use std::cmp::Ordering;

use serde::{Deserialize, Serialize};


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

impl<T: AsRef<str>> PartialEq for Version<T> {
    fn eq(&self, other: &Version<T>) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<T: AsRef<str>> Eq for Version<T> {}

impl<T: AsRef<str>> PartialOrd for Version<T> {
    fn partial_cmp(&self, other: &Version<T>) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: AsRef<str>> Ord for Version<T> {
    fn cmp(&self, other: &Version<T>) -> Ordering {
        use self::Component::*;
        use std::cmp::Ordering::*;
        let mut aiter = self.components();
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
