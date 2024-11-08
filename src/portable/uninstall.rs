use fs_err as fs;
use std::collections::BTreeMap;

use crate::commands::ExitCode;
use crate::platform::{data_dir, portable_dir, tmp_file_path};
use crate::portable::exit_codes;
use crate::portable::local;
use crate::portable::local::InstanceInfo;
use crate::portable::options::Uninstall;
use crate::portable::repository::Query;
use crate::portable::status;
use crate::portable::ver;
use crate::print::{self, Highlight};

pub fn uninstall(options: &Uninstall) -> anyhow::Result<()> {
    let mut candidates = local::get_installed()?;
    if options.nightly {
        candidates.retain(|cand| cand.version.is_nightly());
    }
    if let Some(channel) = options.channel {
        let query = Query::from(channel);
        candidates.retain(|cand| query.matches(&cand.version));
    }
    if let Some(ver) = &options.version {
        if let Ok(ver) = ver.parse::<ver::Filter>() {
            candidates.retain(|cand| ver.matches(&cand.version));
        } else if let Ok(ver) = ver.parse::<ver::Specific>() {
            candidates.retain(|cand| ver == cand.version.specific());
        } else if let Ok(ver) = ver.parse::<ver::Build>() {
            candidates.retain(|cand| ver == cand.version);
        } else {
            anyhow::bail!("cannot parse version {:?}", ver);
        }
    }
    let mut used_versions = BTreeMap::new();
    let data_dir = data_dir()?;
    if data_dir.exists() {
        for pair in status::list_local(&data_dir)? {
            let (name, _) = pair?;
            if let Some(info) = InstanceInfo::try_read(&name)? {
                used_versions.insert(info.get_version()?.specific(), info.name);
            }
        }
    }
    let mut all = true;
    candidates.retain(|cand| {
        if let Some(inst_name) = used_versions.get(&cand.version.specific()) {
            if !options.unused {
                log::warn!("Version {} is used by {:?}", cand.version, inst_name);
            }
            all = false;
            false
        } else {
            true
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
        msg!("Uninstalled {} versions.", uninstalled.emphasize());
        print::error("some instances are in use. See messages above.");
        Err(ExitCode::new(exit_codes::PARTIAL_SUCCESS))?;
    } else if uninstalled > 0 {
        msg!("Successfully uninstalled {} versions.",
            uninstalled.emphasize());
    } else {
        print::success("Nothing to uninstall.")
    }
    Ok(())
}
