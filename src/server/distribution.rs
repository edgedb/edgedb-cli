use serde::{Deserialize, Serialize};
use serde::{ser, de};

use crate::server::version::Version;


pub struct DistributionRef(Box<dyn Distribution>);

pub trait Distribution {
    fn major_ver(&self) -> &MajorVersion;
}

#[derive(PartialEq, PartialOrd, Ord, Eq, Debug, Clone)]
pub enum MajorVersion {
    Stable(Version<String>),
    Nightly,
}

impl MajorVersion {
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
}

impl<'de> Deserialize<'de> for MajorVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: de::Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        match s {
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
