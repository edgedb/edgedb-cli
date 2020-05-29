use std::collections::HashSet;

use crate::server::options::Init;
use crate::server::detect::{self, VersionQuery};


pub fn init(options: &Init) -> Result<(), anyhow::Error> {
    let version_query = VersionQuery::new(
        options.nightly, &options.version);
    let current_os = detect::current_os()?;
    let avail_methods = current_os.get_available_methods()?;
    let (ver, meth) = if let Some(ref method) = options.method {
        todo!();
    } else if version_query.is_nightly() || version_query.is_specific() {
        todo!();
    } else {
        let methods = avail_methods.instantiate_all(&*current_os, true)?;
        let mut max_ver = None;
        let mut ver_methods = HashSet::new();
        for (meth, method) in &methods {
            for ver in method.installed_versions()? {
                if let Some(ref mut max_ver) = max_ver {
                    if *max_ver == ver.major_version {
                        ver_methods.insert(meth.clone());
                    } else if *max_ver < ver.major_version {
                        *max_ver = ver.major_version.clone();
                        ver_methods.clear();
                        ver_methods.insert(meth.clone());
                    }
                } else {
                    max_ver = Some(ver.major_version.clone());
                    ver_methods.insert(meth.clone());
                }
            }
        }
        if let Some(ver) = max_ver {
            let mut ver_methods = ver_methods.into_iter().collect::<Vec<_>>();
            ver_methods.sort();
            let mut methods = methods;
            let meth = methods.remove(&ver_methods.remove(0))
                .expect("method is recently used");
            (ver, meth)
        } else {
            anyhow::bail!("Cannot find any installed version. Run: \n  \
                edgedb server install");
        }
    };
    let system = options.system || meth.is_system_only();
    if system {
        todo!();
    } else {
        // TODO(tailhook)
        // Create a directory
        // Put a method name into a directory
        // Bootstrap edgedb into that directory
        // Create a systemd --user file or not
        todo!();
    };
}
