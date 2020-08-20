use std::io;
use std::process::{Command, Child, exit};

use anyhow::Context;

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
        .context(format!("can decode output of {:?}", cmd))
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
                log::error!("error stopping server: {}",
                    io::Error::last_os_error());
            }
        }
        if cfg!(not(unix)) {
            self.child.kill().map_err(|e| {
                log::error!("error stopping server: {}", e);
            }).ok();
        }

        // TODO(tailhook) figure out what signals and exit codes are okay
        self.child.wait().map_err(|e| {
            log::error!("error waiting for stopped server: {}", e);
        }).ok();
    }
}
