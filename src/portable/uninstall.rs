use std::collections::BTreeMap;
use fs_err as fs;

use crate::portable::local;
use crate::commands::ExitCode;
use crate::portable::exit_codes;
use crate::portable::status;
use crate::portable::ver;
use crate::portable::local::{InstanceInfo};
use crate::platform::{tmp_file_path, data_dir, portable_dir};
use crate::server::options::Uninstall;
use crate::print::{self, echo, Highlight};


pub fn uninstall(options: &Uninstall) -> anyhow::Result<()> {
    if options.deprecated_install_methods {
        return crate::server::uninstall::uninstall(options);
    }
    let mut candidates = local::get_installed()?;
    if options.nightly {
        candidates.retain(|cand| cand.version.is_nightly());
    }
    if let Some(ver) = &options.version {
        if let Ok(ver) = ver.num().parse::<ver::Filter>() {
            candidates.retain(|cand| ver.matches(&cand.version));
        } else if let Ok(ver) = ver.num().parse::<ver::Specific>() {
            candidates.retain(|cand| ver == cand.version.specific());
        } else if let Ok(ver) = ver.num().parse::<ver::Build>() {
            candidates.retain(|cand| ver == cand.version);
        } else {
            anyhow::bail!("cannot parse version {:?}", ver);
        }
    }
    let mut used_versions = BTreeMap::new();
    for pair in status::list_local(&data_dir()?)? {
        let (name, _) = pair?;
        InstanceInfo::try_read(&name)?
            .map(|info| {
                used_versions.insert(info.installation.version.specific(),
                                     info.name);
            });
    }
    let mut all = true;
    candidates.retain(|cand| {
        if let Some(inst_name) = used_versions.get(&cand.version.specific()) {
            if !options.unused {
                log::warn!("Version {} is used by {:?}",
                           cand.version, inst_name);
            }
            all = false;
            return false;
        } else {
            return true;
        }
    });
    let mut uninstalled = 0;
    for cand in candidates {
        log::info!("Uninstalling {}", cand.version);
        let path = portable_dir()?.join(cand.version.specific().to_string());
        let tmp_dir = tmp_file_path(&path);
        if tmp_dir.exists() {
            fs::remove_dir_all(&tmp_dir)?;
        }
        fs::rename(path, &tmp_dir)?;
        fs::remove_dir_all(&tmp_dir)?;
        uninstalled += 1;
    }

    if !all && !options.unused {
        echo!("Uninstalled", uninstalled.emphasize(), "versions.");
        print::error("some instances are used. See messages above.");
        return Err(ExitCode::new(exit_codes::PARTIAL_SUCCESS))?;
    } else if uninstalled > 0 {
        echo!("Successfully uninstalled",
               uninstalled.emphasize(), "versions.");
    } else {
        print::success("Nothing to uninstall.")
    }
    Ok(())
}
