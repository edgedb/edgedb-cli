use std::str::FromStr;

use serde::{Serialize, Deserialize};
use linked_hash_map::LinkedHashMap;

use crate::server::os_trait::{CurrentOs, Method};
use crate::server::package::PackageCandidate;
use crate::server::docker::DockerCandidate;


pub type Methods<'a> = LinkedHashMap<InstallMethod, Box<dyn Method + 'a>>;


#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[derive(Serialize, Deserialize)]
pub enum InstallMethod {
    Package,
    Docker,
}

#[derive(Debug)]
pub struct InstallationMethods {
    pub package: PackageCandidate,
    pub docker: DockerCandidate,
}


impl InstallationMethods {
    pub fn instantiate_all<'x>(&self, os: &'x dyn CurrentOs,
        skip_on_error: bool)
        -> anyhow::Result<Methods<'x>>
    {
        use InstallMethod::*;

        let mut methods = LinkedHashMap::new();
        for meth_name in &[Package, Docker] {
            if self.is_supported(meth_name) {
                match os.make_method(&Package, &self) {
                    Ok(meth) => {
                        methods.insert(Package, meth);
                    }
                    Err(e) if skip_on_error => {
                        eprintln!("WARNING: {:#}", e);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(methods)
    }
    pub fn is_supported(&self, meth: &InstallMethod) -> bool {
        use InstallMethod::*;

        match meth {
            Package => self.package.supported,
            Docker => self.docker.supported,
        }
    }
    pub fn format_error(&self) -> String {
        let mut buf = String::with_capacity(1024);
        if self.package.supported || self.docker.supported {
            buf.push_str("No installation method chosen, add:\n");
            if self.package.supported {
                self.package.format_option(&mut buf, true);
            }
            if self.docker.supported {
                self.docker.format_option(&mut buf, !self.package.supported);
            }
            if !self.package.supported {
                self.package.format_error(&mut buf);
            }
            if !self.docker.supported {
                self.docker.format_error(&mut buf);
            }
            buf.push_str("or run `edgedb server install --interactive` \
                          and follow instructions");
        } else if self.docker.platform_supported {
            buf.push_str("No installation method found:\n");
            self.package.format_error(&mut buf);
            self.docker.format_error(&mut buf);
            buf.push_str("Consider installing docker: \
                https://docs.docker.com/get-docker/");
            buf.push_str("Or ask for native support at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
        } else {
            buf.push_str("No installation method supported for the platform:");
            self.package.format_error(&mut buf);
            self.docker.format_error(&mut buf);
            buf.push_str("Please consider opening an issue at \
                https://github.com/edgedb/edgedb-cli/issues/new\
                ?template=install-unsupported.md");
        }
        return buf;
    }
}

impl FromStr for InstallMethod {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<InstallMethod> {
        match s {
            "package" => Ok(InstallMethod::Package),
            "docker" => Ok(InstallMethod::Docker),
            _ => anyhow::bail!("Unknown installation method {:?}. \
                Options: package, docker"),
        }
    }
}

impl InstallMethod {
    pub fn title(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "Native System Package",
            Docker => "Docker Container",
        }
    }
    pub fn option(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "--method=package",
            Docker => "--method=docker",
        }
    }
    pub fn short_name(&self) -> &'static str {
        use InstallMethod::*;
        match self {
            Package => "package",
            Docker => "docker",
        }
    }
}
