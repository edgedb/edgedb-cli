// Portions Copyright (c) 2020 MagicStack Inc.
// Portions Copyright (c) 2016 The Rust Project Developers.

use std::env;
use std::fs;
use std::io::{Write, BufRead, stdin, stdout};
use std::path::{PathBuf, Path};
use std::process::exit;

use anyhow::Context;
use clap::Clap;
use prettytable::{Table, Row, Cell};

use crate::table;
use crate::platform::{home_dir, get_current_uid};


#[derive(Clap, Clone, Debug)]
pub struct SelfInstall {
    /// Install nightly version of command-line tools
    #[clap(long)]
    pub nightly: bool,
    /// Enable verbose output
    #[clap(short='v', long)]
    pub verbose: bool,
    /// Disable progress output
    #[clap(short='q', long)]
    pub quiet: bool,
    /// Disable confirmation prompt
    #[clap(short='y')]
    pub no_confirm: bool,
    /// Do not configure the PATH environment variable
    #[clap(long)]
    pub no_modify_path: bool,
}

pub struct Settings {
    system: bool,
    installation_path: PathBuf,
    modify_path: bool,
    env_file: PathBuf,
    rc_files: Vec<PathBuf>,
}

fn print_long_description(settings: &Settings) {
    println!(r###"
Welcome to EdgeDB!

This will install the EdgeDB command-line tools.

It will add `edgedb` binary to the {dir_kind} bin directory located at:

  {installation_path}
{profile_update}
"###,
        dir_kind=if settings.system { "system" } else { "user" },
        installation_path=settings.installation_path.display(),
        profile_update=if cfg!(windows) {
            format!(r###"
This path will then be added to your `PATH` environment variable by
modifying the `HKEY_CURRENT_USER/Environment/PATH` registry key.
"###)
        } else if settings.modify_path {
            format!(r###"
This path will then be added to your PATH environment variable by
modifying the profile file{s} located at:

{rc_files}
"###,
            s=if settings.rc_files.len() > 1 { "s" } else { "" },
            rc_files=settings.rc_files.iter()
                     .map(|p| format!("  {}", p.display()))
                     .collect::<Vec<_>>()
                     .join("\n"),
            )
        } else if should_modify_path(&settings.installation_path) {
            format!(r###"
Path {installation_path} should be added to the PATH manually after
installation.
"###,
                installation_path=settings.installation_path.display())
        } else {
            r###"
This path is already in your PATH environment variable, so no profile will
be modified.
"###.into()
        },
    )
}

fn should_modify_path(dir: &Path) -> bool {
    if let Some(all_paths) = env::var_os("PATH") {
        for path in env::split_paths(&all_paths) {
            if path == dir {
                // not needed
                return false;
            }
        }
    }
    return true;
}

fn get_rc_files() -> anyhow::Result<Vec<PathBuf>> {
    let mut rc_files = Vec::new();

    let home_dir = home_dir()?;
    rc_files.push(home_dir.join(".profile"));

    if let Ok(shell) = env::var("SHELL") {
        if shell.contains("zsh") {
            let var = env::var_os("ZDOTDIR");
            let zdotdir = var.as_deref()
                .map_or_else(|| home_dir.as_path(), Path::new);
            let zprofile = zdotdir.join(".zprofile");
            rc_files.push(zprofile);
        }
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
        let text = fs::read_to_string(path)
            .context("cannot read file")?;
        if text.contains(line) {
            return Ok(())
        }
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)
        .context("cannot file for append (writing)")?;
    file.write(format!("{}\n", line).as_bytes(),)
        .context("cannot append to file")?;
    Ok(())
}

fn print_post_install_message(settings: &Settings) {
    if cfg!(windows) {
        print!(r###"# EdgeDB command-line tool is installed now!

We've updated your environment configuration to have {dir} in your `PATH`
environment variable. Next time you open the terminal it will be configured
automatically.
"###,
            dir=settings.installation_path.display());
    } else if settings.modify_path {
        print!(r###"# EdgeDB command-line tool is installed now!

We've updated your shell profile to have {dir} in your `PATH`
environment variable. Next time you open the terminal it will be configured
automatically.

For this session please run:
  source {env_path}
"###,
            dir=settings.installation_path.display(),
            env_path=settings.env_file.display());
    } else {
        println!(r###"EdgeDB command-line tool is installed now!"###);
    }
    println!("\nTo install the server locally run:\n  \
                edgedb server install");
}

pub fn read_choice() -> anyhow::Result<String> {
    for line in stdin().lock().lines() {
        let line = line.context("reading user input")?;
        return Ok(line.trim().to_lowercase())
    }
    anyhow::bail!("Unexpected end of input");
}
pub fn main(options: &SelfInstall) -> anyhow::Result<()> {
    match _main(options) {
        Ok(()) => {
            if cfg!(windows) && !options.no_confirm {
                // This is needed so user can read the message if console
                // was open just for this process
                eprintln!("Press the Enter key to continue");
                read_choice()?;
            }
            Ok(())
        }
        Err(e) => {
            if cfg!(windows) && !options.no_confirm {
                // This is needed so user can read the message if console
                // was open just for this process
                eprintln!("Error: {:#}", e);
                eprintln!("Press the Enter key to continue");
                read_choice()?;
                exit(1);
            }
            Err(e)
        }
    }
}

fn customize(settings: &mut Settings) -> anyhow::Result<()> {
    if should_modify_path(&settings.installation_path) {
        loop {
            print!("Modify PATH variable? (Y/n)");

            stdout().flush()?;
            match read_choice()?.as_ref() {
                "y" | "yes" | "" => {
                    settings.modify_path = true;
                    break;
                }
                "n" | "no" => {
                    settings.modify_path = false;
                    break;
                }
                choice => {
                    eprintln!("Invalid choice {:?}. \
                        Use single letter `y` or `n`.",
                        choice);
                }
            }
        }
    } else {
        println!("No options to customize");
    }
    Ok(())
}

fn _main(options: &SelfInstall) -> anyhow::Result<()> {
    let mut settings = if !cfg!(windows) && get_current_uid() == 0 {
        anyhow::bail!("Installation as root is not supported. \
            Try running without sudo.")
    } else {
        let base = home_dir()?.join(".edgedb");
        let installation_path = base.join("bin");
        Settings {
            rc_files: get_rc_files()?,
            system: false,
            modify_path: !options.no_modify_path &&
                         should_modify_path(&installation_path),
            installation_path,
            env_file: base.join("env"),
        }
    };
    if !options.quiet {
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
                        eprintln!("Aborting installation");
                        exit(7);
                    }
                }
            }
        }
    }

    let tmp_path = settings.installation_path.join(".edgedb.tmp");
    let path = if cfg!(windows) {
        settings.installation_path.join("edgedb.exe")
    } else {
        settings.installation_path.join("edgedb")
    };
    let exe_path = env::current_exe()
        .with_context(|| format!("cannot determine running executable path"))?;
    fs::create_dir_all(&settings.installation_path)
        .with_context(|| format!("failed to create {:?}",
                                 settings.installation_path))?;
    fs::remove_file(&tmp_path).ok();
    fs::copy(&exe_path, &tmp_path)
        .with_context(|| format!("failed to write {:?}", tmp_path))?;
    fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to rename {:?}", tmp_path))?;

    if settings.modify_path {
        #[cfg(windows)] {
            windows_add_to_path(&settings.installation_path)
                .context("failed adding a directory to PATH")?;
        }
        if cfg!(unix) {
            let line = format!("\nexport PATH=\"{}:$PATH\"",
                               settings.installation_path.display());
            for path in &settings.rc_files {
                ensure_line(&path, &line)
                    .with_context(|| format!("failed to update profile file {:?}",
                                             path))?;
            }
            fs::write(&settings.env_file, &(line + "\n"))
                .context("failed to write env file")?;
        }
    }

    print_post_install_message(&settings);

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
                       Not modifying the PATH variable");
                return Ok(None);
            }
        }
        Err(ref e) if e.kind() == io::ErrorKind::NotFound => Ok(Some(String::new())),
        Err(e) => Err(e).context("windows failure"),
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
fn windows_add_to_path(installation_path: &Path) -> anyhow::Result<()> {
    use std::ptr;
    use std::env::{join_paths, split_paths};
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

    if old_path.iter().any(|p| p == installation_path) {
        return Ok(());
    }

    let new_path = join_paths(vec![installation_path].into_iter()
                              .chain(old_path.iter().map(|x| x.as_ref())))
            .context("can't join path")?;
    let new_path = new_path.to_str()
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

impl Settings {
    pub fn print(&self) {
        let mut table = Table::new();
        table.add_row(Row::new(vec![
            Cell::new("Instalation Path"),
            Cell::new(&self.installation_path.display().to_string()),
        ]));
        table.add_row(Row::new(vec![
            Cell::new("Modify PATH Variable"),
            Cell::new(if self.modify_path { "yes" } else { "no" }),
        ]));
        if self.modify_path && !self.rc_files.is_empty() {
            table.add_row(Row::new(vec![
                Cell::new("Profile Files"),
                Cell::new(&self.rc_files.iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")),
            ]));
        }
        table.set_format(*table::FORMAT);
        table.printstd();
    }
}
