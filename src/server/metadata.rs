use serde::{Deserialize, Serialize};
use serde::de;
use serde_json::Value;

use crate::server::version::Version;
use crate::server::distribution::{MajorVersion};
use crate::server::methods::InstallMethod;
use crate::server::options::StartConf;


#[derive(Debug, PartialEq, Serialize, Clone)]
#[serde(into="MetadataV2")]
pub struct Metadata {
    pub version: MajorVersion,
    pub slot: Option<String>,
    pub current_version: Option<Version<String>>,
    pub method: InstallMethod,
    pub port: u16,
    pub start_conf: StartConf,
}

#[derive(Serialize, Deserialize)]
pub struct MetadataV2 {
    format: u16,
    version: MajorVersion,
    #[serde(default, skip_serializing_if="Option::is_none")]
    current_version: Option<Version<String>>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    slot: Option<String>,
    method: InstallMethod,
    port: u16,
    start_conf: StartConf,
}

#[derive(Deserialize, Debug)]
pub struct MetadataV1 {
    #[serde(default="two")]
    format: u16,
    version: Version<String>,
    method: InstallMethod,
    port: u16,
    nightly: bool,
    start_conf: StartConf,
}

fn two() -> u16 {
    2
}

impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = Value::deserialize(deserializer)?;
        match Option::deserialize(&v["format"]).map_err(de::Error::custom)? {
            None | Some(1) => {
                Ok(MetadataV1::deserialize(v)
                    .map_err(de::Error::custom)?
                    .into())
            }
            Some(2) => {
                Ok(MetadataV2::deserialize(v)
                    .map_err(de::Error::custom)?
                    .into())
            }
            Some(ver) => {
                Err(de::Error::custom(
                    format!("unsupported metadata format {}", ver)))
            }
        }
    }
}

impl From<MetadataV1> for Metadata {
    fn from(m: MetadataV1) -> Metadata {
        Metadata {
            slot: Some(m.version.as_ref().into()),
            version: if m.nightly {
                MajorVersion::Nightly
            } else {
                MajorVersion::Stable(m.version)
            },
            current_version: None,
            method: m.method,
            port: m.port,
            start_conf: m.start_conf,
        }
    }
}

impl From<MetadataV2> for Metadata {
    fn from(m: MetadataV2) -> Metadata {
        Metadata {
            version: m.version,
            slot: m.slot,
            current_version: m.current_version,
            method: m.method,
            port: m.port,
            start_conf: m.start_conf,
        }
    }
}

impl From<Metadata> for MetadataV2 {
    fn from(m: Metadata) -> MetadataV2 {
        MetadataV2 {
            format: 2,
            version: m.version,
            slot: m.slot,
            current_version: m.current_version,
            method: m.method,
            port: m.port,
            start_conf: m.start_conf,
        }
    }
}

#[cfg(test)]
mod test {
    use super::Metadata;
    use crate::server::version::Version;
    use crate::server::distribution::{MajorVersion};
    use crate::server::methods::InstallMethod;
    use crate::server::options::StartConf;

    #[test]
    fn old_metadata() {
        assert_eq!(serde_json::from_str::<Metadata>(r###"
            {"version":"1-alpha5","method":"Package","port":10700,
             "nightly":false,"start_conf":"Auto"}
        "###).unwrap(), Metadata {
            version: MajorVersion::Stable(Version("1-alpha5".into())),
            current_version: None,
            slot: Some("1-alpha5".into()),
            method: InstallMethod::Package,
            port: 10700,
            start_conf: StartConf::Auto,
        });

        assert_eq!(serde_json::from_str::<Metadata>(r###"
            {"version":"1-alpha6","method":"Package","port":10700,
             "nightly":true,"start_conf":"Auto"}
        "###).unwrap(), Metadata {
            version: MajorVersion::Nightly,
            current_version: None,
            slot: Some("1-alpha6".into()),
            method: InstallMethod::Package,
            port: 10700,
            start_conf: StartConf::Auto,
        });
    }

    #[test]
    fn new_metadata() {
        assert_eq!(serde_json::to_string_pretty(&Metadata {
            version: MajorVersion::Stable(Version("1-alpha5".into())),
            current_version: None,
            slot: Some("1-alpha5".into()),
            method: InstallMethod::Package,
            port: 10700,
            start_conf: StartConf::Auto,
        }).unwrap(), r###"{
  "format": 2,
  "version": "1-alpha5",
  "slot": "1-alpha5",
  "method": "Package",
  "port": 10700,
  "start_conf": "Auto"
}"###);

        assert_eq!(serde_json::to_string_pretty(&Metadata {
            version: MajorVersion::Nightly,
            current_version: Some(Version("1a3.dev.g124bc".into())),
            slot: Some("1-alpha6".into()),
            method: InstallMethod::Package,
            port: 10700,
            start_conf: StartConf::Auto,
        }).unwrap(), r###"{
  "format": 2,
  "version": "nightly",
  "current_version": "1a3.dev.g124bc",
  "slot": "1-alpha6",
  "method": "Package",
  "port": 10700,
  "start_conf": "Auto"
}"###);
    }
}
