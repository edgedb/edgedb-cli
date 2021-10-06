use std::borrow::Cow;
use std::ffi::OsStr;
use std::future::pending;
use std::io;
use std::path::Path;

use anyhow::Context;
use async_process::Command;
use async_std::prelude::FutureExt;
use async_std::task;

use crate::interrupt;


pub struct Native {
    command: Command,
    marker: Cow<'static, str>,
    description: Cow<'static, str>,
}

impl Native {
    pub fn new(description: impl Into<Cow<'static, str>>,
        marker: impl Into<Cow<'static, str>>,
        cmd: impl AsRef<Path>)
        -> Native
    {
        Native {
            description: description.into(),
            marker: marker.into(),
            command: Command::new(cmd.as_ref()),
        }
    }
    pub fn marker(&mut self, marker: impl Into<Cow<'static, str>>) -> &mut Self
    {
        self.marker = marker.into();
        self
    }
    pub fn descr(&mut self, description: impl Into<Cow<'static, str>>)
        -> &mut Self
    {
        self.description = description.into();
        self
    }
    pub fn arg(&mut self, val: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(val);
        self
    }
    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>)
        -> &mut Self
    {
        self.command.env(key, val);
        self
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        task::block_on(self._run())
    }

    async fn _run(&mut self) -> anyhow::Result<()> {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id();
        let child_result = child.status()
            .race(async { process_loop(pid, &term, &self.description).await })
            .await;
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to run {} (command-line: {:?})",
                self.description, self.command))?;
        if status.success() {
            log::debug!("Result of {}: {}", self.description, status);
            Ok(())
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, status, self.command);
        }
    }
}

#[cfg(windows)]
async fn process_loop(_: u32, _: &interrupt::Interrupt, _: &str) -> !
{
    // on windows Ctrl+C signals are propagated automatically and no other
    // signals are supported, so there is nothing to do here
    pending::<()>().await;
    unreachable!();
}

#[cfg(unix)]
async fn process_loop(pid: u32, intr: &interrupt::Interrupt, descr: &str)
    -> !
{
    use async_std::future::timeout;
    use signal_hook::consts::signal::{SIGTERM, SIGKILL};
    use std::time::Duration;

    let sig = intr.wait().await;
    match sig {
        interrupt::Signal::Interrupt => {
            log::warn!("Got interrupt. Waiting for \
                the {} process to exit.", descr);
        }
        interrupt::Signal::Hup => {
            log::warn!("Got HUP signal. Waiting for \
                the {} process to exit.", descr);
        }
        interrupt::Signal::Term => {
            log::warn!("Got TERM signal. Propagating to {}...", descr);
            if unsafe { libc::kill(pid as i32, SIGTERM) } != 0 {
                log::debug!("Error signalling process: {}",
                    io::Error::last_os_error());
            }
        }
    };
    timeout(Duration::from_secs(10), pending::<()>()).await.ok();
    log::warn!("Process {} did not stop in 10 seconds, forcing...", descr);
    unsafe { libc::kill(pid as i32, SIGKILL) };
    pending::<()>().await;
    unreachable!();
}
