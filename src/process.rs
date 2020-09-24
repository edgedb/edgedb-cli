use std::io;
use std::process::{Command, Child, exit};
use std::time::Duration;

use anyhow::Context;
use serde::de::DeserializeOwned;
use wait_timeout::ChildExt;


pub struct ProcessGuard {
    child: Child,
}


pub fn run(cmd: &mut Command) -> anyhow::Result<()> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => anyhow::bail!("process {:?} failed: {}", cmd, s),
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd)),
    }
}

pub fn run_or_stderr(cmd: &mut Command) -> anyhow::Result<Result<(), String>> {
    match cmd.output() {
        Ok(child) if child.status.success() => Ok(Ok(())),
        Ok(out) => {
            Ok(Err(String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("can decode error output of {:?}", cmd)
                })?))
        }
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd)),
    }
}

pub fn exit_from(cmd: &mut Command) -> anyhow::Result<()> {
    match cmd.status() {
        Ok(s) if s.code().is_some() => exit(s.code().unwrap()),
        Ok(s) => anyhow::bail!("process {:?} failed: {}", cmd, s),
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd)),
    }
}

pub fn get_text(cmd: &mut Command) -> anyhow::Result<String> {
    let data = match cmd.output() {
        Ok(out) if out.status.success() => out.stdout,
        Ok(out) => anyhow::bail!("process {:?} failed: {}", cmd, out.status),
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd))?,
    };
    String::from_utf8(data)
        .with_context(|| format!("can decode output of {:?}", cmd))
}

pub fn get_json_or_stderr<T: DeserializeOwned>(cmd: &mut Command)
    -> anyhow::Result<Result<T, String>>
{
    match cmd.output() {
        Ok(out) if out.status.success() => {
            Ok(Ok(serde_json::from_slice(&out.stdout[..])
                .with_context(|| format!("can decode output of {:?}", cmd))?))
        }
        Ok(out) => {
            Ok(Err(String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("can decode error output of {:?}", cmd)
                })?))
        }
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd))?,
    }
}

impl ProcessGuard {
    pub fn run(cmd: &mut Command) -> anyhow::Result<ProcessGuard> {
        Ok(ProcessGuard {
            child: cmd.spawn()?,
        })
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
         #[cfg(unix)] {
             let pid = self.child.id() as i32;
             if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
                 log::error!("error stopping command: {}",
                     io::Error::last_os_error());
             }
         }
         if cfg!(not(unix)) {
             self.child.kill().map_err(|e| {
                 log::error!("error stopping command: {}", e);
             }).ok();
         }
         match self.child.wait_timeout(Duration::from_secs(10)) {
             Ok(None) => {
                 self.child.kill().map_err(|e| {
                     log::warn!("error stopping command: {}", e);
                 }).ok();
                 self.child.wait().map_err(|e| {
                     log::error!("error waiting for stopped command: {}", e);
                 }).ok();
             }
             Ok(Some(_)) => {}
             Err(e) => {
                 log::error!("error stopping command: {}", e);
             }
         }
    }
}
