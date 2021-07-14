use std::env;
use std::path::Path;
use std::io;

use edgedb_cli_derive::EdbClap;
use fs_err as fs;

use crate::credentials;
use crate::platform::{home_dir, tmp_file_path, symlink_dir, config_dir};
use crate::project;
use crate::question;
use crate::print_markdown;
use crate::commands::ExitCode;
use crate::self_upgrade::binary_path;


#[derive(EdbClap, Clone, Debug)]
pub struct SelfMigrate {
    /// Dry run: do no actually move anything
    #[clap(short='v', long)]
    pub verbose: bool,

    /// Dry run: do no actually move anything (use with increased verbosity)
    #[clap(short='n', long)]
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
enum ConfirmOverwrite {
    Yes,
    Skip,
    Quit,
}

pub fn main(options: &SelfMigrate) -> anyhow::Result<()> {
    let base = home_dir()?.join(".edgedb");
    if !base.exists() {
        log::warn!("Directory {:?} does not exists. Nothing to do.", base);
    }
    migrate(&base, options.dry_run)
}

fn file_is_non_empty(path: &Path) -> anyhow::Result<bool> {
    match fs::metadata(path) {
        Ok(meta) => Ok(meta.len() > 0),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn dir_is_non_empty(path: &Path) -> anyhow::Result<bool> {
    match fs::read_dir(path) {
        Ok(mut dir) => Ok(dir.next().is_some()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn move_file(src: &Path, dest: &Path, dry_run: bool) -> anyhow::Result<()> {
    use ConfirmOverwrite::*;

    if file_is_non_empty(&dest)? {
        if dry_run {
            log::warn!("File {:?} exists in both locations, \
                        will prompt for overwrite", dest);
            return Ok(());
        }
        let mut q = question::Choice::new(format!(
            "Attempting to move {:?} > {:?}, but \
            destination file exists. Do you want to overwrite?",
            src, dest));
        q.option(Yes, &["y"], "overwrite the destination file");
        q.option(Skip, &["s"],
            "skip, keep the destination file, remove the source");
        q.option(Quit, &["q"], "quit now without overwriting");
        match q.ask()? {
            Yes => {},
            Skip => return Ok(()),
            Quit => anyhow::bail!("Cancelled by user"),
        }
    } else {
        if dry_run {
            log::info!("Would move {:?} -> {:?}", src, dest);
            return Ok(());
        }
    }
    let tmp = tmp_file_path(&dest);
    fs::copy(src, &tmp)?;
    fs::rename(tmp, dest)?;
    fs::remove_file(src)?;
    Ok(())
}

fn move_dir(src: &Path, dest: &Path, dry_run: bool) -> anyhow::Result<()> {
    use ConfirmOverwrite::*;

    if dir_is_non_empty(&dest)? {
        if dry_run {
            log::warn!("Directory {:?} exists in both locations, \
                        will prompt for overwrite", dest);
            return Ok(());
        }
        let mut q = question::Choice::new(format!(
            "Attempting to move {:?} > {:?}, but \
            destination directory exists. Do you want to overwrite?",
            src, dest));
        q.option(Yes, &["y"], "overwrite the destination dir");
        q.option(Skip, &["s"],
            "skip, keep the destination dir, remove the source");
        q.option(Quit, &["q"], "quit now without overwriting");
        match q.ask()? {
            Yes => {},
            Skip => return Ok(()),
            Quit => anyhow::bail!("Cancelled by user"),
        }
    } else {
        if dry_run {
            log::info!("Would move {:?} -> {:?}", src, dest);
            return Ok(());
        }
    }
    fs::create_dir_all(dest)?;
    for item in fs::read_dir(src)? {
        let item = item?;
        let ref dest_path = dest.join(item.file_name());
        match item.file_type()? {
            typ if typ.is_file() => {
                let tmp = tmp_file_path(dest_path);
                fs::copy(item.path(), &tmp)?;
                fs::rename(&tmp, dest_path)?;
            }
            #[cfg(unix)]
            typ if typ.is_symlink() => {
                let path = fs::read_link(item.path())?;
                symlink_dir(path, dest_path)
                    .map_err(|e| {
                        log::info!(
                            "Error symlinking project at {:?}: {}",
                            dest_path, e);
                    }).ok();
            }
            _ => {
                log::warn!("Skipping {:?} of unexpected type", item.path());
            }
        }
    }
    fs::remove_dir_all(src)?;
    Ok(())
}

fn try_move_bin(exe_path: &Path) -> anyhow::Result<()> {
    let bin_path = binary_path()?;
    let bin_dir = bin_path.parent().unwrap();
    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir)?;
    }
    fs::rename(&exe_path, &bin_path)?;
    Ok(())
}

pub fn migrate(base: &Path, dry_run: bool) -> anyhow::Result<()> {
    if let Ok(exe_path) = env::current_exe() {
        if exe_path.starts_with(base) {
            try_move_bin(&exe_path)
            .map_err(|e| {
                eprintln!("Cannot move executable to the new location. \
                    Try `edgedb self upgrade` instead");
                e
            })?;
        }
    }

    let source = base.join("credentials");
    let target = credentials::base_dir()?;
    if source.exists() {
        if !dry_run {
            fs::create_dir_all(&target)?;
        }
        for item in fs::read_dir(&source)? {
            let item = item?;
            move_file(&item.path(), &target.join(item.file_name()), dry_run)?;
        }
        if !dry_run {
            fs::remove_dir(&source)
                .map_err(|e| log::warn!("Cannot remove {:?}: {}", source, e))
                .ok();
        }
    }

    let source = base.join("projects");
    let target = project::stash_base()?;
    if source.exists() {
        if !dry_run {
            fs::create_dir_all(&target)?;
        }
        for item in fs::read_dir(&source)? {
            let item = item?;
            if item.metadata()?.is_dir() {
                move_dir(&item.path(),
                         &target.join(item.file_name()), dry_run)?;
            }
        }
        if !dry_run {
            fs::remove_dir(&source)
                .map_err(|e| log::warn!("Cannot remove {:?}: {}", source, e))
                .ok();
        }
    }

    let source = base.join("config");
    let target = config_dir()?;
    if source.exists() {
        if !dry_run {
            fs::create_dir_all(&target)?;
        }
        for item in fs::read_dir(&source)? {
            let item = item?;
            move_file(&item.path(), &target.join(item.file_name()), dry_run)?;
        }
        if !dry_run {
            fs::remove_dir(&source)
                .map_err(|e| log::warn!("Cannot remove {:?}: {}", source, e))
                .ok();
        }
    }

    remove_file(&base.join("env"), dry_run)?;
    remove_dir_all(&base.join("bin"), dry_run)?;

    if cfg!(target_os="macos") {
        macos_recreate_all_services(dry_run)?;
    }

    remove_dir_all(&base.join("run"), dry_run)?;
    remove_dir_all(&base.join("logs"), dry_run)?;
    remove_dir_all(&base.join("cache"), dry_run)?;

    if !dry_run && dir_is_non_empty(&base)? {
        eprintln!("\
            Directory {:?} is not used by EdgeDB tools any more and must be \
            removed to finish migration. But there are some files or \
            directories left after all known files moved to the locations. \
            This might be because third party tools left some files there. \
        ", base);
        let q = question::Confirm::new(format!(
            "Do you want to remove all files and directories within {:?}?",
            base,
        ));
        if !q.ask()? {
            eprintln!("edgedb error: Cancelled by user");
            print_markdown!("\
                When all files are backed up, just run either of:\n\
                ```\n\
                rm -rf ~/.edgedb\n\
                edgedb self migrate\n\
                ```\
            ");
            return Err(ExitCode::new(2).into());
        }
    }
    remove_dir_all(&base, dry_run)?;

    Ok(())
}

fn remove_file(path: &Path, dry_run: bool) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(())
    }
    if dry_run {
        log::info!("Would remove {:?}", path);
        return Ok(());
    }
    log::info!("Removing {:?}", path);
    fs::remove_file(path)?;
    Ok(())
}

fn remove_dir_all(path: &Path, dry_run: bool) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(())
    }
    if dry_run {
        log::info!("Would remove dir {:?} recursively", path);
        return Ok(());
    }
    log::info!("Removing dir {:?}", path);
    fs::remove_dir_all(path)?;
    Ok(())
}

/// Recreates all service files to update /run and /logs dirs
fn macos_recreate_all_services(dry_run: bool) -> anyhow::Result<()> {
    use crate::server::detect;
    use crate::server::methods::InstallMethod;
    use crate::server::options::{Start, Stop, StartConf};
    use crate::server::macos;

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let meth = os.make_method(&InstallMethod::Package, &avail_methods)?;

    for inst in meth.all_instances()? {
        if dry_run {
            log::info!("Would restart instance {:?}", inst.name());
            continue;
        }
        log::info!("Stopping instance {:?}", inst.name());
        inst.stop(&Stop { name: inst.name().into() })?;
        log::info!("Updating service file for instance {:?}", inst.name());
        macos::recreate_launchctl_service(inst.name(), inst.get_meta()?)?;
        if inst.get_start_conf()? == StartConf::Auto {
            log::info!("Starting instance {:?}", inst.name());
            inst.start(&Start {
                name: inst.name().into(),
                foreground: false,
            })?;
        } else {
            log::warn!(
                "Service {:?} is not started due to `--start-conf=manual`",
                inst.name(),
            );
        }
    }

    Ok(())
}
