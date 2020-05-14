use std::process::{Command as StdCommand, Stdio, ExitStatus};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context as ContextExt;


#[derive(Debug)]
pub struct Command {
    cmd: PathBuf,
    arguments: Vec<OsString>,
}

#[derive(Debug)]
pub enum Operation {
    FeedPrivilegedCmd {
        input: Vec<u8>,
        cmd: Command,
    },
    WritePrivilegedFile {
        path: PathBuf,
        data: Vec<u8>,
    },
    PrivilegedCmd(Command),
}

pub struct Context {
    sudo_cmd: Option<PathBuf>,
}

impl Context {
    pub fn new() -> Context {
        Context {
            sudo_cmd: None,
        }
    }
    pub fn set_elevation_cmd(&mut self, path: &Path) {
        self.sudo_cmd = Some(path.into());
    }
}

impl Command {
    pub fn new(cmd: impl Into<PathBuf>) -> Command {
        Command {
            cmd: cmd.into(),
            arguments: Vec::new(),
        }
    }
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.arguments.push(arg.into());
        self
    }
}

fn cmd_result(status: Result<ExitStatus, io::Error>, cmd: StdCommand)
    -> Result<(), anyhow::Error>
{
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(anyhow::anyhow!("Command {:?} {}", cmd, s)),
        Err(e) => Err(e).context(format!("Command {:?} error", cmd)),
    }
}

fn tmp_filename(path: &Path) -> PathBuf {
    const SUFFIX: &str = ".edgedb-server-install.tmp";

    let mut buf = PathBuf::from(path.parent().expect("full path"));
    let name = path.file_name().expect("path with filename");
    let mut name_buf = OsString::with_capacity(name.len() + 2 + SUFFIX.len());
    name_buf.push(".~");
    name_buf.push(name);
    name_buf.push(SUFFIX);
    buf.push(name_buf);
    return buf;
}

impl Operation {
    pub fn is_privileged(&self) -> bool {
        use Operation::*;
        matches!(self,
            FeedPrivilegedCmd {..}
            | WritePrivilegedFile {..}
            | PrivilegedCmd(..)
        )
    }
    pub fn format(&self, elevate: bool) -> String {
        use Operation::*;
        use std::fmt::Write;

        let mut buf = String::new();
        match self {
            FeedPrivilegedCmd {cmd, ..} | PrivilegedCmd(cmd) => {
                if elevate {
                    buf.push_str("sudo ");
                }
                write!(&mut buf, "{}", cmd.cmd.display()).unwrap();
                for arg in &cmd.arguments {
                    write!(&mut buf, " {}", arg.to_string_lossy()).unwrap();
                }
            }
            WritePrivilegedFile { path, .. } => {
                if elevate {
                    buf.push_str("sudo ");
                }
                write!(&mut buf, "tee {}", path.display()).unwrap();
            }
        }
        buf
    }
    pub fn perform(&self, ctx: &Context)
        -> Result<(), anyhow::Error>
    {
        use Operation::*;

        match self {
            FeedPrivilegedCmd {cmd, input} => {
                let mut os_cmd = if let Some(sudo_cmd) = &ctx.sudo_cmd {
                    let mut os_cmd = StdCommand::new(sudo_cmd);
                    os_cmd.arg(&cmd.cmd);
                    os_cmd
                } else {
                    StdCommand::new(&cmd.cmd)
                };
                for arg in &cmd.arguments {
                    os_cmd.arg(arg);
                }
                os_cmd.stdin(Stdio::piped());
                let mut child = os_cmd.spawn()
                    .with_context(|| format!("Command {:?} error", os_cmd))?;
                child.stdin.as_mut().unwrap().write_all(input)
                    .with_context(|| format!("Command {:?} error", os_cmd))?;
                log::info!("Executing {:?}", os_cmd);
                cmd_result(child.wait(), os_cmd)
            }
            PrivilegedCmd(cmd) => {
                let mut os_cmd = if let Some(sudo_cmd) = &ctx.sudo_cmd {
                    let mut os_cmd = StdCommand::new(sudo_cmd);
                    os_cmd.arg(&cmd.cmd);
                    os_cmd
                } else {
                    StdCommand::new(&cmd.cmd)
                };
                for arg in &cmd.arguments {
                    os_cmd.arg(arg);
                }
                log::info!("Executing {:?}", os_cmd);
                cmd_result(os_cmd.status(), os_cmd)
            }
            WritePrivilegedFile { path, data } => {
                if ctx.sudo_cmd.is_some() {
                    FeedPrivilegedCmd {
                        cmd: Command::new("tee").arg(path),
                        input: data.clone(),
                    }.perform(ctx)
                } else {
                    log::info!("Writing {:?}", path);
                    let tmpname = tmp_filename(path);
                    fs::remove_file(&tmpname).ok();
                    fs::write(&tmpname, data)?;
                    fs::rename(&tmpname, path)?;
                    Ok(())
                }
            }
        }
    }
}
