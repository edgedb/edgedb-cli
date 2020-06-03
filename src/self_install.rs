use std::env;
use std::fs;
use std::io::Write;
use std::path::{PathBuf, Path};

use anyhow::Context;
use clap::Clap;
use dirs::{home_dir, executable_dir};
use prettytable::{Table, Row, Cell};
use users::get_current_uid;

use crate::table;


#[derive(Clap, Clone, Debug)]
pub struct SelfInstall {
    /// Install nightly version of command-line tools
    #[clap(long)]
    pub nightly: bool,
    /// Enable verbose output
    #[clap(short="v", long)]
    pub verbose: bool,
    /// Disable progress output
    #[clap(short="q", long)]
    pub quiet: bool,
    /// Disable confirmation prompt
    #[clap(short="y")]
    pub no_confirm: bool,
    /// Do not configure the PATH environment variable
    #[clap(long)]
    pub no_modify_path: bool,
}

pub struct Settings {
    system: bool,
    installation_path: PathBuf,
    modify_path: bool,
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
Path {installation_path} should be added to ath PATH manually after
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

    let home_dir = home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
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
        print!(r###"# EdgeDB command-line tool is installed now. Great!

To get started you need installation directory ({dir}) in your `PATH`
environment variable. Future applications will automatically have the
correct environment, but you may need to restart your current shell.
"###,
            dir=settings.installation_path.display());
    } else if settings.modify_path {
        print!(r###"# EdgeDB command-line tool is installed now. Great!

To get started you need installation directory ({dir}) in your `PATH`
environment variable. Next time you log in this will be done
automatically.

To configure your current shell run `export PATH="{dir}:$PATH"`
"###,
            dir=settings.installation_path.display());
    } else {
        println!(r###"EdgeDB command-line tool is installed now. Great!"###);
    }
}

pub fn main(options: &SelfInstall) -> anyhow::Result<()> {
    let settings = if cfg!(windows) {
        let installation_path = home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home dir"))?
            .join("EdgeDB/CLI");
        Settings {
            system: false,
            modify_path: !options.no_modify_path &&
                         should_modify_path(&installation_path),
            installation_path,
            rc_files: Vec::new(),
        }
    } else if get_current_uid() == 0 {
        Settings {
            system: true,
            modify_path: false,
            installation_path: PathBuf::from("/usr/bin"),
            rc_files: Vec::new(),
        }
    } else {
        let installation_path = if cfg!(target_os="macos") {
            PathBuf::from("/usr/local/bin")
        } else {
            executable_dir()
            .ok_or_else(|| {
                anyhow::anyhow!("Cannot determine executable dir")
            })?
        };
        Settings {
            rc_files: get_rc_files()?,
            system: false,
            modify_path: !options.no_modify_path &&
                         should_modify_path(&installation_path),
            installation_path,
        }
    };
    if !options.quiet {
        print_long_description(&settings);
        settings.print();
        if !options.no_confirm {
            // TODO(tailhook) ask confirmation
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
            windows_add_to_path(&settings.installation_path);
        }
        #[cfg(unix)] {
            let line = format!("\nexport PATH=\"{}:$PATH\"",
                               settings.installation_path.display());
            for path in &settings.rc_files {
                ensure_line(&path, &line)
                    .with_context(|| format!("failed to update profile file {:?}",
                                             path))?;
            }
        }
    }

    print_post_install_message(&settings);

    Ok(())
}

#[cfg(windows)]
fn windows_add_to_path(installation_path: &Path) -> Result<()> {
    anyhow::bail!("adding to PATH for windows is not implemented yet");
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
        if self.modify_path {
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
