// Portions Copyright (c) 2020 MagicStack Inc.
// Portions Copyright (c) 2016 The Rust Project Developers.

use std::env;
use std::fs;
use std::io::{stdout, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::str::FromStr;

use anyhow::Context;
use clap_complete::{generate, shells};
use edgedb_tokio::get_stash_path;
use fn_error_context::context;
use prettytable::{Cell, Row, Table};

use crate::branding::{BRANDING, BRANDING_CLI, BRANDING_CLI_CMD};
use crate::cli::{migrate, upgrade};
use crate::commands::ExitCode;
use crate::options::Options;
use crate::platform::current_exe;
use crate::platform::{binary_path, config_dir, home_dir};
use crate::portable::platform;
use crate::portable::project::project_dir;
use crate::portable::project::{self, Init};
use crate::print::{self};
use crate::print_markdown;
use crate::process;
use crate::question::{self, read_choice};
use crate::table;

#[derive(clap::Parser, Clone, Debug)]
pub struct CliInstall {
    #[arg(long, hide = true)]
    pub nightly: bool,
    #[arg(long, hide = true)]
    pub testing: bool,
    /// Enable verbose output
    #[arg(short = 'v', long)]
    pub verbose: bool,
    /// Skip printing messages and confirmation prompts
    #[arg(short = 'q', long)]
    pub quiet: bool,
    /// Disable confirmation prompt, also disables running `project init`
    #[arg(short = 'y')]
    pub no_confirm: bool,
    /// Do not configure PATH environment variable
    #[arg(long)]
    pub no_modify_path: bool,
    /// Indicate that edgedb-init should not issue a
    /// "Press Enter to continue" prompt before exiting
    /// on Windows. Used when edgedb-init is invoked
    /// from an existing terminal session and not in
    /// a new window.
    #[arg(long)]
    pub no_wait_for_exit_prompt: bool,

    /// Installation is run from `self upgrade` command
    #[arg(long, hide = true)]
    pub upgrade: bool,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "kebab-case")]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

#[derive(clap::Args, Clone, Debug)]
pub struct GenCompletions {
    /// Shell to print out completions for
    #[arg(long)]
    pub shell: Option<Shell>,

    /// Install all completions into the prefix
    #[arg(long, conflicts_with = "shell")]
    pub prefix: Option<PathBuf>,

    /// Install all completions into the prefix
    #[arg(long, conflicts_with = "shell", conflicts_with = "prefix")]
    pub home: bool,
}

pub struct Settings {
    system: bool,
    installation_path: PathBuf,
    modify_path: bool,
    env_file: PathBuf,
    rc_files: Vec<PathBuf>,
}

fn print_long_description(settings: &Settings) {
    println!();
    print_markdown!("\
        # Welcome to EdgeDB!\n\
        \n\
        This will install the official EdgeDB command-line tools.\n\
        \n\
        The `edgedb` binary will be placed in the ${dir_kind} bin directory \
        located at:\n\
        ```\n\
            ${installation_path}\n\
        ```\n\
        \n\
        ${update_win\n\
        This path will then be added to your `PATH` environment variable by \
        modifying the `HKEY_CURRENT_USER/Environment/PATH` registry key.\n\
        \n\
        }\n\
        ${update_files\n\
        This path will then be added to your `PATH` environment variable by \
        modifying the profile file${s} located at:\n\
        ```\n\
            ${rc_files}\n\
        ```\n\
        \n\
        }\n\
        ${modify_path
        Path ${installation_path} should be added to the `PATH` manually \
        after installation.\n\
        \n\
        }\n\
        ${no_modified\n\
        This path is already in your `PATH` environment variable, so no \
        profile will be modified.\n\
        }\n\
        ",
        dir_kind=if settings.system { "system" } else { "user" },
        installation_path=settings.installation_path.display(),
        update_win: if cfg!(windows) && settings.modify_path,
        update_files: if !cfg!(windows) && settings.modify_path => {
            rc_files=settings.rc_files.iter()
                     .map(|p| p.display().to_string())
                     .collect::<Vec<_>>()
                     .join("\n"),
            s=if settings.rc_files.len() > 1 { "s" } else { "" },
        },
        modify_path: if !cfg!(windows) && !settings.modify_path &&
                        no_dir_in_path(&settings.installation_path)
        => {
            installation_path=settings.installation_path.display()
        },
        no_modified: if !cfg!(windows) && !settings.modify_path &&
                        !no_dir_in_path(&settings.installation_path),
    );
}

pub fn no_dir_in_path(dir: &Path) -> bool {
    if let Some(all_paths) = env::var_os("PATH") {
        for path in env::split_paths(&all_paths) {
            if path == dir {
                // not needed
                return false;
            }
        }
    }
    true
}

fn is_zsh() -> bool {
    if let Ok(shell) = env::var("SHELL") {
        return shell.contains("zsh");
    }
    false
}

pub fn get_rc_files() -> anyhow::Result<Vec<PathBuf>> {
    let mut rc_files = Vec::new();

    let home_dir = home_dir()?;
    rc_files.push(home_dir.join(".profile"));

    if is_zsh() {
        let var = env::var_os("ZDOTDIR");
        let zdotdir = var.as_deref().map_or_else(|| home_dir.as_path(), Path::new);
        let zprofile = zdotdir.join(".zprofile");
        rc_files.push(zprofile);
    }

    let bash_profile = home_dir.join(".bash_profile");
    // Only update .bash_profile if it exists because creating .bash_profile
    // will cause .profile to not be read
    if bash_profile.exists() {
        rc_files.push(bash_profile);
    }

    Ok(rc_files)
}

fn ensure_line(path: &PathBuf, line: &str) -> anyhow::Result<()> {
    if path.exists() {
        let text = fs::read_to_string(path).context("cannot read file")?;
        if text.contains(line) {
            return Ok(());
        }
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .context("cannot open file for appending (writing)")?;
    file.write(format!("{line}\n").as_bytes())
        .context("cannot append to file")?;
    Ok(())
}

fn print_post_install_message(settings: &Settings, init_result: anyhow::Result<InitResult>) {
    if cfg!(windows) && settings.modify_path {
        print_markdown!(
            "\
            # The EdgeDB command-line tool is now installed!\n\
            \n\
            The `${dir}` directory has been added to your `PATH`. You may\n\
            need to reopen the terminal for this change to take effect\n\
            and for the `edgedb` command to become available.\
            ",
            dir = settings.installation_path.display(),
        );
    } else if settings.modify_path {
        print_markdown!(
            "\
            # The EdgeDB command-line tool is now installed!\n\
            \n\
            Your shell profile has been updated with ${dir} in your `PATH`.\n\
            It will be configured automatically the next time you open the terminal.\n\
            \n\
            For this session please run:\n\
            ```\n\
                source \"${env_path}\"\n\
            ```\
            ",
            dir = settings.installation_path.display(),
            env_path = settings.env_file.display(),
        );
    } else {
        print_markdown!(
            "\
            # The EdgeDB command-line tool is now installed!\
        "
        );
    }
    if is_zsh() {
        let fpath = process::Native::new(
            "zsh",
            "zsh",
            env::var("SHELL").unwrap_or_else(|_| "zsh".into()),
        )
        .arg("-ic")
        .arg("echo $fpath")
        .get_stdout_text()
        .ok();
        let func_dir = home_dir().ok().map(|p| p.join(".zfunc"));
        let func_dir = func_dir.as_ref().and_then(|p| p.to_str());
        if let Some((fpath, func_dir)) = fpath.zip(func_dir) {
            if !fpath.split(' ').any(|s| s == func_dir) {
                print_markdown!(
                    "\n\
                    To enable zsh completion, add:\n\
                    ```\n\
                        fpath+=~/.zfunc\n\
                    ```\n\
                    to your `~/.zshrc` before the `compinit` command.\
                "
                );
            }
        }
    }
    match init_result {
        Ok(InitResult::Initialized) => {
            print_markdown!(
                "\n\
                `edgedb` without parameters will automatically\n\
                connect to the initialized project.\n\
            "
            );
        }
        Ok(InitResult::Refused | InitResult::NonInteractive) => {
            print_markdown!(
                "\n\
                To initialize a new project, run:\n\
                ```\n\
                    ${cmd} project init\n\
                ```\
            ",
                cmd = BRANDING_CLI_CMD
            );
        }
        Ok(InitResult::NotAProject) => {
            print_markdown!(
                "\n\
                To initialize a new project, run:\n\
                ```\n\
                    ${cmd} project init\n\
                ```\
            ",
                cmd = BRANDING_CLI_CMD,
            );
        }
        Ok(InitResult::Already) => {
            print_markdown!(
                "\n\
                `${cmd}` without parameters will automatically\n\
                connect to the current project.\n\
            ",
                cmd = BRANDING_CLI_CMD,
            );
        }
        Ok(InitResult::OldLayout) => {
            print_markdown!(
                "\n\
                To initialize a project run:\n\
                ```\n\
                    ${cmd} cli migrate\n\
                    ${cmd} project init\n\
                ```\
            ",
                cmd = BRANDING_CLI_CMD,
            );
        }
        Err(e) => {
            print_markdown!(
                "
                **There was an error while initializing the project: ${err}**\n\
                \n\
                To restart project initialization, run:\n\
                ```\n\
                    ${cmd} project init\n\
                ```\
                ",
                cmd = BRANDING_CLI_CMD,
                err = format!("{:#}", e),
            );
        }
    }
}

pub fn main(options: &CliInstall) -> anyhow::Result<()> {
    match _main(options) {
        Ok(()) => {
            if cfg!(windows)
                && !options.upgrade
                && !options.no_confirm
                && !options.no_wait_for_exit_prompt
            {
                // This is needed so user can read the message if console
                // was open just for this process
                eprintln!("Press the Enter key to continue");
                read_choice()?;
            }
            Ok(())
        }
        Err(e) => {
            if cfg!(windows)
                && !options.upgrade
                && !options.no_confirm
                && !options.no_wait_for_exit_prompt
            {
                // This is needed so user can read the message if console
                // was open just for this process
                eprintln!("edgedb error: {e:#}");
                eprintln!("Press the Enter key to continue");
                read_choice()?;
                exit(1);
            }
            Err(e)
        }
    }
}

fn customize(settings: &mut Settings) -> anyhow::Result<()> {
    if no_dir_in_path(&settings.installation_path) {
        let q = question::Confirm::new("Modify PATH variable?");
        settings.modify_path = q.ask()?;
    } else {
        print::error("No options to customize.");
    }
    Ok(())
}

pub enum InitResult {
    Initialized,
    Already,
    Refused,
    NonInteractive,
    NotAProject,
    OldLayout,
}

fn try_project_init(new_layout: bool) -> anyhow::Result<InitResult> {
    use InitResult::*;

    let base_dir = env::current_dir().context("failed to get current directory")?;
    if base_dir.parent().is_none() {
        // can't initialize project in root dir
        return Ok(NotAProject);
    }

    let base_dir = env::current_dir().context("failed to get current directory")?;
    if let Some((project_dir, config_path)) = project_dir(Some(&base_dir))? {
        if get_stash_path(&project_dir)?.exists() {
            log::info!("Project already initialized. Skipping...");
            return Ok(Already);
        }
        if !new_layout {
            log::warn!(
                "Directory layout not upgraded; \
            project will not be initialized."
            );
            return Ok(OldLayout);
        }
        println!("Command-line tools are installed successfully.");
        println!();
        let q = question::Confirm::new(format!(
            "Do you want to initialize a new {BRANDING} server instance for the project \
             defined in `{}`?",
            config_path.display(),
        ));
        if !q.ask()? {
            return Ok(Refused);
        }

        let options = crate::options::CloudOptions {
            cloud_api_endpoint: None,
            cloud_secret_key: None,
            cloud_profile: None,
        };
        let init = Init {
            project_dir: None,
            server_version: None,
            server_instance: None,
            database: None,
            non_interactive: false,
            no_migrations: false,
            link: false,
            server_start_conf: None,
            cloud_opts: options.clone(),
        };
        project::init_existing(&init, &project_dir, config_path, &options)?;
        Ok(Initialized)
    } else {
        Ok(NotAProject)
    }
}

fn _main(options: &CliInstall) -> anyhow::Result<()> {
    #[cfg(unix)]
    if !options.no_confirm {
        match home_dir_from_passwd().zip(env::var_os("HOME")) {
            Some((passwd, env)) if passwd != env => {
                msg!("$HOME differs from euid-obtained home directory: \
                       you may be using sudo");
                msg!("$HOME directory: {}", Path::new(&env).display());
                msg!("euid-obtained home directory: {}", passwd.display());
                msg!("if this is what you want, \
                       restart the installation with `-y'");
                return Err(ExitCode::new(1).into());
            }
            _ => {}
        }
    }
    let installation_path = binary_path()?.parent().unwrap().to_owned();
    let mut settings = Settings {
        rc_files: get_rc_files()?,
        system: false,
        modify_path: !options.no_modify_path && no_dir_in_path(&installation_path),
        installation_path,
        env_file: config_dir()?.join("env"),
    };
    if !options.quiet && !options.upgrade {
        print_long_description(&settings);
        settings.print();
        if !options.no_confirm {
            loop {
                println!("1) Proceed with installation (default)");
                println!("2) Customize installation");
                println!("3) Cancel installation");
                match read_choice()?.as_ref() {
                    "" | "1" => break,
                    "2" => {
                        customize(&mut settings)?;
                        settings.print();
                    }
                    _ => {
                        print::error("Aborting installation.");
                        exit(7);
                    }
                }
            }
        }
    }

    if cfg!(all(target_os = "macos", target_arch = "x86_64")) && platform::is_arm64_hardware() {
        msg!("{BRANDING} now supports native M1 build. Downloading binary...");
        return upgrade::upgrade_to_arm64();
    }

    let tmp_path = settings.installation_path.join(".edgedb.tmp");
    let path = if cfg!(windows) {
        settings.installation_path.join("edgedb.exe")
    } else {
        settings.installation_path.join("edgedb")
    };
    let exe_path = current_exe()?;
    fs::create_dir_all(&settings.installation_path)
        .with_context(|| format!("failed to create {:?}", settings.installation_path))?;
    if exe_path.parent() == path.parent() {
        fs::rename(&exe_path, &path).with_context(|| format!("failed to rename {exe_path:?}"))?;
    } else {
        fs::remove_file(&tmp_path).ok();
        fs::copy(&exe_path, &tmp_path).with_context(|| format!("failed to write {tmp_path:?}"))?;
        fs::rename(&tmp_path, &path).with_context(|| format!("failed to rename {tmp_path:?}"))?;
    }
    write_completions_home()?;

    if settings.modify_path {
        #[cfg(windows)]
        {
            use std::env::join_paths;

            windows_augment_path(|orig_path| {
                if orig_path.iter().any(|p| p == &settings.installation_path) {
                    return None;
                }
                Some(
                    join_paths(
                        vec![&settings.installation_path]
                            .into_iter()
                            .chain(orig_path.iter()),
                    )
                    .expect("paths can be joined"),
                )
            })?;
        }
        if cfg!(unix) {
            let line = format!(
                "\nexport PATH=\"{}:$PATH\"",
                settings.installation_path.display()
            );
            for path in &settings.rc_files {
                ensure_line(path, &line)
                    .with_context(|| format!("failed to update profile file {path:?}"))?;
            }
            if let Some(dir) = settings.env_file.parent() {
                fs::create_dir_all(dir).with_context(|| format!("failed to create {dir:?}"))?;
            }
            fs::write(&settings.env_file, line + "\n")
                .with_context(|| format!("failed to write env file {:?}", settings.env_file))?;
        }
    }

    let base = home_dir()?.join(".edgedb");
    let new_layout = if base.exists() {
        eprintln!(
            "\
                {BRANDING_CLI} no longer uses '{}' to store data \
                and now uses standard locations of your OS. \
        ",
            base.display()
        );
        let q = question::Confirm::new(format!(
            "\
            Do you want to run `edgedb cli migrate` now to update \
            the directory layout?\
        "
        ));
        if q.ask()? {
            migrate::migrate(&base, false)?;
            true
        } else {
            false
        }
    } else {
        true
    };

    if !options.upgrade {
        let init_result = if options.no_confirm {
            Ok(InitResult::NonInteractive)
        } else {
            try_project_init(new_layout)
        };

        print_post_install_message(&settings, init_result);
    }

    Ok(())
}

// This is used to decode the value of HKCU\Environment\PATH. If that
// key is not unicode (or not REG_SZ | REG_EXPAND_SZ) then this
// returns null.  The winreg library itself does a lossy unicode
// conversion.
#[cfg(windows)]
pub fn string_from_winreg_value(val: &winreg::RegValue) -> Option<String> {
    use std::slice;
    use winreg::enums::RegType;

    match val.vtype {
        RegType::REG_SZ | RegType::REG_EXPAND_SZ => {
            // Copied from winreg
            let words = unsafe {
                #[allow(clippy::cast_ptr_alignment)]
                slice::from_raw_parts(val.bytes.as_ptr().cast::<u16>(), val.bytes.len() / 2)
            };

            String::from_utf16(words).ok().and_then(|mut s| {
                while s.ends_with('\u{0}') {
                    s.pop();
                }
                Some(s)
            })
        }
        _ => None,
    }
}

#[cfg(windows)]
// Get the windows PATH variable out of the registry as a String. If
// this returns None then the PATH variable is not unicode and we
// should not mess with it.
fn get_windows_path_var() -> anyhow::Result<Option<String>> {
    use std::io;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let environment = root
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("permission denied")?;

    let reg_value = environment.get_raw_value("PATH");
    match reg_value {
        Ok(val) => {
            if let Some(s) = string_from_winreg_value(&val) {
                Ok(Some(s))
            } else {
                log::warn!("the registry key HKEY_CURRENT_USER\\Environment\\PATH does not contain valid Unicode. \
                       PATH variable will not be modified.");
                return Ok(None);
            }
        }
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(Some(String::new())),
        Err(e) => Err(e).context("Windows failure"),
    }
}

/// Encodes a utf-8 string as a null-terminated UCS-2 string in bytes
#[cfg(windows)]
pub fn string_to_winreg_bytes(s: &str) -> Vec<u8> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let v: Vec<u16> = OsStr::new(s).encode_wide().chain(Some(0)).collect();
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), v.len() * 2).to_vec() }
}

#[cfg(windows)]
pub fn windows_augment_path<F: FnOnce(&[PathBuf]) -> Option<std::ffi::OsString>>(
    f: F,
) -> anyhow::Result<()> {
    use std::env::{join_paths, split_paths};
    use std::ptr;
    use winapi::shared::minwindef::*;
    use winapi::um::winuser::SendMessageTimeoutA;
    use winapi::um::winuser::{HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    use winreg::enums::{RegType, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::{RegKey, RegValue};

    let old_path: Vec<_> = if let Some(s) = get_windows_path_var()? {
        split_paths(&s).collect()
    } else {
        // Non-unicode path
        return Ok(());
    };
    let new_path = match f(&old_path) {
        Some(path) => path,
        None => return Ok(()),
    };

    let new_path = new_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("failed to convert PATH to utf-8"))?;

    let root = RegKey::predef(HKEY_CURRENT_USER);
    let environment = root
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("permission denied")?;

    let reg_value = RegValue {
        bytes: string_to_winreg_bytes(&new_path),
        vtype: RegType::REG_EXPAND_SZ,
    };

    environment
        .set_raw_value("PATH", &reg_value)
        .context("permission denied")?;

    // Tell other processes to update their environment

    unsafe {
        SendMessageTimeoutA(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            "Environment\0".as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            ptr::null_mut(),
        );
    }
    Ok(())
}

#[context("writing completion file {:?}", path)]
fn write_completion(path: &Path, shell: Shell) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    shell.generate(&mut BufWriter::new(fs::File::create(path)?));
    Ok(())
}

pub fn write_completions_home() -> anyhow::Result<()> {
    let home = home_dir()?;
    write_completion(
        &home.join(".local/share/bash-completion/completions/edgedb"),
        Shell::Bash,
    )?;
    write_completion(
        &home.join(".config/fish/completions/edgedb.fish"),
        Shell::Fish,
    )?;
    write_completion(&home.join(".zfunc/_edgedb"), Shell::Zsh)?;
    Ok(())
}

pub fn gen_completions(options: &GenCompletions) -> anyhow::Result<()> {
    if let Some(shell) = options.shell {
        shell.generate(&mut stdout());
    } else if let Some(prefix) = &options.prefix {
        write_completion(
            &prefix.join("share/bash-completion/completions/edgedb"),
            Shell::Bash,
        )?;
        write_completion(
            &prefix.join("share/fish/completions/edgedb.fish"),
            Shell::Fish,
        )?;
        write_completion(&prefix.join("share/zsh/site-functions/_edgedb"), Shell::Zsh)?;
    } else if options.home {
        write_completions_home()?;
    } else {
        anyhow::bail!("either `--prefix` or `--shell=` is expected");
    }
    Ok(())
}

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Installation Path"),
            Cell::new(&self.installation_path.display().to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Modify PATH Variable"),
            Cell::new(if self.modify_path { "yes" } else { "no" }),
        ]));
        if self.modify_path && !self.rc_files.is_empty() {
            table.add_row(Row::new(vec![
                Cell::new("Profile Files"),
                Cell::new(
                    &self
                        .rc_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            ]));
        }
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}

impl FromStr for Shell {
    type Err = anyhow::Error;
    fn from_str(v: &str) -> anyhow::Result<Shell> {
        use Shell::*;
        match v {
            "bash" => Ok(Bash),
            "elvish" => Ok(Elvish),
            "fish" => Ok(Fish),
            "powershell" => Ok(PowerShell),
            "zsh" => Ok(Zsh),
            _ => anyhow::bail!("unknown shell {:?}", v),
        }
    }
}

impl Shell {
    fn generate(&self, buf: &mut dyn Write) {
        use Shell::*;

        let mut app = Options::command();
        let n = "edgedb";
        match self {
            Bash => generate(shells::Bash, &mut app, n, buf),
            Elvish => generate(shells::Elvish, &mut app, n, buf),
            Fish => generate(shells::Fish, &mut app, n, buf),
            PowerShell => generate(shells::PowerShell, &mut app, n, buf),
            Zsh => generate(shells::Zsh, &mut app, n, buf),
        }
    }
}

// search user database to get home dir of euid user
#[cfg(unix)]
pub(crate) fn home_dir_from_passwd() -> Option<PathBuf> {
    use std::ffi::{CStr, OsString};
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStringExt;
    use std::ptr;
    unsafe {
        let init_size = match libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) {
            -1 => 1024,
            n => n as usize,
        };
        let mut buf = Vec::with_capacity(init_size);
        let mut pwd: MaybeUninit<libc::passwd> = MaybeUninit::uninit();
        let mut pwdp = ptr::null_mut();
        match libc::getpwuid_r(
            libc::geteuid(),
            pwd.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.capacity(),
            &mut pwdp,
        ) {
            0 if !pwdp.is_null() => {
                let pwd = pwd.assume_init();
                let bytes = CStr::from_ptr(pwd.pw_dir).to_bytes().to_vec();
                let pw_dir = OsString::from_vec(bytes);
                Some(PathBuf::from(pw_dir))
            }
            _ => None,
        }
    }
}
