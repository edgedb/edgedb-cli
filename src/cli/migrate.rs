use std::env;
use std::path::{Path, PathBuf};
use std::io::{self, Write};

use anyhow::Context;
use edgedb_cli_derive::EdbClap;
use fs_err as fs;
use fn_error_context::context;

use crate::cli::install::{get_rc_files, no_dir_in_path};
use crate::credentials;
use crate::platform::binary_path;
use crate::platform::{home_dir, tmp_file_path, symlink_dir, config_dir};
use crate::print;
use crate::project;
use crate::question;
use crate::print_markdown;
use crate::commands::ExitCode;


#[derive(EdbClap, Clone, Debug)]
pub struct CliMigrate {
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

pub fn main(options: &CliMigrate) -> anyhow::Result<()> {
    let base = home_dir()?.join(".edgedb");
    if base.exists() {
        migrate(&base, options.dry_run)
    } else {
        log::warn!("Directory {:?} does not exist. Nothing to do.", base);
        Ok(())
    }
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
            "Attempting to move {:?} -> {:?}, but \
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
            "Attempting to move {:?} -> {:?}, but \
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

fn try_move_bin(exe_path: &Path, bin_path: &Path) -> anyhow::Result<()> {
    let bin_dir = bin_path.parent().unwrap();
    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir)?;
    }
    fs::rename(&exe_path, &bin_path)?;
    Ok(())
}

#[context("error updating {:?}", path)]
fn replace_line(path: &PathBuf, old_line: &str, new_line: &str)
    -> anyhow::Result<bool>
{
    if !path.exists() {
        return Ok(false);
    }
    let text = fs::read_to_string(path)
        .context("cannot read file")?;
    if let Some(idx) = text.find(old_line) {
        log::info!("File {:?} contains old path, replacing", path);
        let mut file = fs::File::create(path)?;
        file.write(text[..idx].as_bytes())?;
        file.write(new_line.as_bytes())?;
        file.write(text[idx+old_line.len()..].as_bytes())?;
        Ok(true)
    } else {
        log::info!("File {:?} has no old path, skipping", path);
        return Ok(false);
    }
}

fn update_path(base: &Path, new_bin_path: &Path) -> anyhow::Result<()> {
    log::info!("Updating PATH");
    let old_bin_dir = base.join("bin");
    let new_bin_dir = new_bin_path.parent().unwrap();
    #[cfg(windows)] {
        use std::env::join_paths;

        let mut modified = false;
        crate::cli::install::windows_augment_path(|orig_path| {
            if orig_path.iter().any(|p| p == new_bin_dir) {
                return None;
            }
            Some(join_paths(
                orig_path.iter()
                .map(|x| {
                    if x == &old_bin_dir {
                        modified = true;
                        new_bin_dir
                    } else {
                        x.as_ref()
                    }
                })
           ).expect("paths can be joined"))
        })?;
        if modified && no_dir_in_path(&new_bin_dir) {
            print::success("The `edgedb` executable has moved!");
            print_markdown!("\
                \n\
                We've updated your environment configuration to have\n\
                `${dir}` in your `PATH` environment variable. You\n\
                may need to reopen the terminal for this change to\n\
                take effect, and for the `edgedb` command to become\n\
                available.\
                ",
                dir=new_bin_dir.display(),
            );
        }
    }
    if cfg!(unix) {
        let rc_files = get_rc_files()?;
        let old_line = format!(
            "\nexport PATH=\"{}:$PATH\"\n",
            old_bin_dir.display(),
        );
        let new_line = format!(
            "\nexport PATH=\"{}:$PATH\"\n",
            new_bin_dir.display(),
        );
        let mut modified = false;
        for path in &rc_files {
            if replace_line(&path, &old_line, &new_line)? {
                modified = true;
            }
        }

        let cfg_dir = config_dir()?;
        let env_file = cfg_dir.join("env");

        fs::create_dir_all(&cfg_dir)
            .with_context(
                || format!("failed to create {:?}", cfg_dir))?;
        fs::write(&env_file, &(new_line + "\n"))
            .with_context(
                || format!("failed to write env file {:?}", env_file))?;

        if modified && no_dir_in_path(&new_bin_dir) {
            print::success("The `edgedb` executable has moved!");
            print_markdown!("\
                \n\
                We've updated your shell profile to have ${dir} in your\n\
                `PATH` environment variable. Next time you open the terminal\n\
                it will be configured automatically.\n\
                \n\
                For this session please run:\n\
                ```\n\
                    source \"${env_path}\"\n\
                ```\n\
                Depending on your shell type you might also need \
                to run `rehash`.\
                ",
                dir=new_bin_dir.display(),
                env_path=env_file.display(),
            );
        }
    }
    Ok(())
}

pub fn migrate(base: &Path, dry_run: bool) -> anyhow::Result<()> {
    if let Ok(exe_path) = env::current_exe() {
        if exe_path.starts_with(base) {
            let new_bin_path = binary_path()?;
            try_move_bin(&exe_path, &new_bin_path)
            .map_err(|e| {
                eprintln!("Cannot move executable to the new location. \
                    Try `edgedb cli upgrade` instead");
                e
            })?;
            update_path(base, &new_bin_path)?;
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
            print::error_msg("edgedb error", "Cancelled by user.");
            print_markdown!("\
                When all files are backed up, run either of:\n\
                ```\n\
                rm -rf ~/.edgedb\n\
                edgedb cli migrate\n\
                ```\
            ");
            return Err(ExitCode::new(2).into());
        }
    }
    remove_dir_all(&base, dry_run)?;
    print::success("Directory layout migration successful!");

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
    use crate::server::macos;

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let meth = os.make_method(&InstallMethod::Package, &avail_methods)?;

    for inst in meth.all_instances()? {
        if dry_run {
            log::info!("Would restart instance {:?}", inst.name());
            continue;
        }
        macos::recreate_launchctl_service(inst)?;
    }

    Ok(())
}
