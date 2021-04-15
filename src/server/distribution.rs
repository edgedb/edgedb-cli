use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde::{ser, de};

use crate::server::version::Version;
use crate::server::detect::VersionQuery;


#[derive(Debug, Clone)]
pub struct DistributionRef(Arc<dyn Distribution>);

pub trait Distribution: downcast_rs::DowncastSync + fmt::Debug {
    fn major_version(&self) -> &MajorVersion;
    fn version(&self) -> &Version<String>;
    fn into_ref(self) -> DistributionRef where Self: Sized {
        DistributionRef(Arc::new(self))
    }
}

downcast_rs::impl_downcast!(Distribution);

#[derive(PartialEq, PartialOrd, Ord, Eq, Debug, Clone)]
pub enum MajorVersion {
    Stable(Version<String>),
    Nightly,
}

impl MajorVersion {
    pub fn is_nightly(&self) -> bool {
        matches!(self, MajorVersion::Nightly)
    }
    pub fn as_stable(&self) -> Option<&Version<String>> {
        match self {
            MajorVersion::Stable(v) => Some(v),
            MajorVersion::Nightly => None,
        }
    }
    pub fn option(&self) -> String {
        match self {
            MajorVersion::Stable(v) => format!("--version={}", v.num()),
            MajorVersion::Nightly => "--nightly".into(),
        }
    }
    pub fn title(&self) -> &str {
        match self {
            MajorVersion::Stable(v) => v.num(),
            MajorVersion::Nightly => "nightly",
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            MajorVersion::Stable(v) => v.num(),
            MajorVersion::Nightly => "nightly",
        }
    }
    pub fn to_query(&self) -> VersionQuery {
        match self {
            MajorVersion::Stable(v) => VersionQuery::Stable(Some(v.clone())),
            MajorVersion::Nightly => VersionQuery::Nightly,
        }
    }
}

impl<'de> Deserialize<'de> for MajorVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        match &s[..] {
            "nightly" => Ok(MajorVersion::Nightly),
            s => Ok(MajorVersion::Stable(Version(s.into()))),
        }
    }
}

impl Serialize for MajorVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        serializer.serialize_str(match self {
            MajorVersion::Stable(ver) => ver.num(),
            MajorVersion::Nightly => "nightly",
        })
    }
}

impl DistributionRef {
    pub fn major_version(&self) -> &MajorVersion {
        self.0.major_version()
    }
    pub fn version(&self) -> &Version<String> {
        self.0.version()
    }
    pub fn downcast_ref<T: Distribution>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}
