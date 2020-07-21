use serde::{Serialize, Deserialize};

use crate::server::version::Version;
use crate::server::detect::{Lazy, InstalledPackage, VersionQuery};
use crate::server::detect::{VersionResult};
use crate::server::os_trait::CurrentOs;


#[derive(Debug, Serialize)]
pub struct PackageCandidate {
    pub supported: bool,
    pub distro_name: String,
    pub distro_version: String,
    pub distro_supported: bool,
    pub version_supported: bool,
}

#[derive(Debug, Serialize)]
pub struct PackageMethod<'os, O: CurrentOs + ?Sized> {
    #[serde(skip)]
    pub os: &'os O,
    #[serde(skip)]
    pub installed: Lazy<Vec<InstalledPackage>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RepositoryInfo {
    pub packages: Vec<PackageInfo>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PackageInfo {
    pub basename: String,
    pub slot: Option<Version<String>>,
    pub version: Version<String>,
    pub revision: String,
    pub architecture: String,
}

impl PackageCandidate {
    pub fn format_option(&self, buf: &mut String, recommended: bool) {
        use std::fmt::Write;

        write!(buf, " * --method=package -- to install {} native package",
            self.distro_name).unwrap();
        if recommended {
            buf.push_str(" (recommended)");
        }
        buf.push('\n');
    }

    pub fn format_error(&self, buf: &mut String) {
        use std::fmt::Write;

        if self.distro_supported {
            write!(buf,
                " * Note: native packages are not supported for {} {}",
                self.distro_name,
                self.distro_version).unwrap();
        } else {
            buf.push_str(" * Note: native packages are \
                             not supported for this platform");
        }
        buf.push('\n');
    }
    pub fn make_method<'os, O>(&self, os: &'os O)
        -> anyhow::Result<PackageMethod<'os, O>>
        where O: CurrentOs + ?Sized,
    {
        if !self.supported {
            anyhow::bail!("Method `package` is not supported");
        }
        Ok(PackageMethod {
            os,
            installed: Lazy::lazy(),
        })
    }
}

fn version_matches(package: &PackageInfo, version: &VersionQuery) -> bool {
    use VersionQuery::*;

    if package.slot.is_none() ||
        (package.basename != "edgedb" && package.basename != "edgedb-server")
    {
        return false;
    }
    match version {
        Nightly => true,
        Stable(None) => true,
        Stable(Some(v)) => package.slot.as_ref() == Some(v),
    }
}


pub fn find_version(haystack: &RepositoryInfo, ver: &VersionQuery)
    -> Result<VersionResult, anyhow::Error>
{
    let mut max_version = None::<(&PackageInfo, Version<String>)>;
    for package in &haystack.packages {
        if version_matches(package, ver) {
            let cur_version = package.full_version();
            if let Some((_, max_ver)) = &max_version {
                if max_ver < &cur_version {
                    max_version = Some((package, cur_version));
                }
            } else {
                max_version = Some((package, cur_version));
            }
        }
    }
    if let Some((target, _)) = max_version {
        let major = target.slot.as_ref().unwrap().clone();
        Ok(VersionResult {
            package_name:
                if major.to_ref() >= Version("1-alpha3") {
                    "edgedb-server".into()
                } else {
                    "edgedb".into()
                },
            major_version: major,
            version: target.version.clone(),
            revision: target.revision.clone(),
        })
    } else {
        anyhow::bail!("Version {} not found", ver)
    }
}

impl PackageInfo {
    pub fn full_version(&self) -> Version<String> {
        Version(format!("{}-{}", self.version, self.revision))
    }
}

#[cfg(test)]
mod test {
    use super::RepositoryInfo;
    use super::Version;
    use super::VersionQuery;
    use super::VersionResult;
    use super::find_version;

    #[test]
    fn test_find_version() {
        let json_contents = r#"
            {"packages": [{"architecture": "x86_64",
                           "basename": "edgedb-server",
                           "installref": "/archive/macos-x86_64/edgedb-server-1-alpha3_1.0a3_2020060201.pkg",
                           "name": "edgedb-server-1-alpha3",
                           "revision": "2020060201",
                           "slot": "1-alpha3",
                           "version": "1.0a3"},
                          {"architecture": "x86_64",
                           "basename": "edgedb-server",
                           "installref": "/archive/macos-x86_64/edgedb-server-1-alpha4_1.0a4_2020071512.pkg",
                           "name": "edgedb-server-1-alpha4",
                           "revision": "2020071512",
                           "slot": "1-alpha4",
                           "version": "1.0a4"},
                          {"architecture": "x86_64",
                           "basename": "edgedb-server",
                           "installref": "/archive/macos-x86_64/edgedb-server-1-alpha4_1.0a4_2020071614.pkg",
                           "name": "edgedb-server-1-alpha4",
                           "revision": "2020071614",
                           "slot": "1-alpha4",
                           "version": "1.0a4"}]}
        "#;
        let repository_info: RepositoryInfo = serde_json::from_str(json_contents).unwrap();
        let version_str = format!("{}-{}", "1.0a4", "2020071614");
        let query = VersionQuery::Stable(Some(Version(version_str.to_owned())));
        let result = find_version(&repository_info, &query);
        let expected = VersionResult {
            package_name: String::from("edgedb-server"),
            major_version: Version(version_str.to_owned()),
            version: Version(version_str.to_owned()),
            revision: String::from("2020071614"),
        };
        match result {
            Ok(actual) => assert!(actual.package_name == expected.package_name &&
                                  actual.version == expected.version &&
                                  actual.revision == expected.revision),
            Err(_) => (),
        };
    }
}