use crate::commands::ExitCode;
use crate::print;
use crate::server::detect::{self, VersionQuery};
use crate::server::options::Uninstall;
use crate::server::distribution::MajorVersion;


pub fn uninstall(options: &Uninstall) -> Result<(), anyhow::Error> {
    let os = detect::current_os()?;
    let methods = os.get_available_methods()?.instantiate_all(&*os, true)?;
    let mut all = true;
    let mut uninstalled = 0;
    for meth in methods.values() {
        let mut candidates = if let Some(ver) = &options.version {
            vec![meth.get_version(&VersionQuery::Stable(Some(ver.clone())))?]
        } else {
            meth.installed_versions()?
        };
        if options.nightly {
            candidates.retain(|cand| cand.major_version().is_nightly());
        }
        let instances = meth.all_instances()?;
        for inst in instances {
            let major = inst.get_version()?;
            let exact = inst.get_current_version()?;
            candidates.retain(|cand| {
                let del = match (cand.major_version(), major) {
                    (MajorVersion::Nightly, MajorVersion::Nightly)
                    => Some(cand.version()) == exact,
                    (MajorVersion::Stable(a), MajorVersion::Stable(b))
                    => a == b,
                    _ => false,
                };
                if del && !options.unused {
                    log::warn!("Version {} is used by {:?}",
                        cand.version(), inst.name());
                    all = false;
                }
                return !del;
            });
            if candidates.is_empty() {
                break;
            }
        }
        if candidates.is_empty() && options.unused {
            log::info!("{}: All instances are used. Nothing to uninstall",
                meth.name().title());
            continue;
        }
        for cand in candidates {
            log::info!("{}: Uninstalling {}",
                meth.name().title(), cand.version());
            meth.uninstall(&cand)?;
            uninstalled += 1;
        }
    }
    if !all {
        print::error("some instances are used. See messages above.");
        return Err(ExitCode::new(2))?;
    } else if uninstalled > 0 {
        print::success(
            format!("Successfully uninstalled {} versions.", uninstalled)
        );
    } else {
        print::success("Nothing to uninstall.")
    }
    Ok(())
}
