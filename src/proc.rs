use std::borrow::Cow;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::Context;

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
    #[cfg(unix)]
    pub fn run(&mut self) -> anyhow::Result<()> {
        use signal::Signal::*;

        // note the Trap uses sigmask, so it overrides signal handlers
        let trap = signal::trap::Trap::trap(
            &[SIGINT, SIGTERM, SIGHUP, SIGCHLD]);

        log::info!("Running {}: {:?}", self.description, self.command);
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id() as i32;
        let mut interrupted = false;
        let mut terminated = false;
        let status = 'child: loop {
            for sig in trap {
                match sig {
                    SIGTERM|SIGHUP => {
                        log::warn!("Got {:?} signal. Propagating...",
                                   sig);
                        if unsafe { libc::kill(pid, sig as libc::c_int) } != 0 {
                            log::debug!("Error signalling process: {}",
                                io::Error::last_os_error());
                        }
                        terminated = true;
                    }
                    SIGINT => {
                        log::warn!("Interrupted. Waiting for \
                            child process to exit.");
                        interrupted = true;
                    }
                    _ => {}
                }
                if let Some(status) = child.try_wait()? {
                    break 'child status;
                }
            }
            unreachable!();
        };
        log::debug!("Result of {}: {}", self.description, status);
        if terminated {
            interrupt::exit_on_sigterm();
        }
        if interrupted {
            interrupt::exit_on_sigint();
        }
        if status.success() {
            return Ok(())
        }
        anyhow::bail!("{} failed: {} (command-line: {:?})",
            self.description, status, self.command);
    }
    #[cfg(windows)]
    pub fn run(&mut self) -> anyhow::Result<()> {
        self._run_on_windows()
    }
    #[allow(dead_code)]
    fn _run_windows(&mut self) -> anyhow::Result<()> {
        // mask out CtrlC, only for this process, so child gets interrupt
        let ctrlc = interrupt::CtrlC::new();

        log::info!("Running {}: {:?}", self.description, self.command);
        let status = self.command.status()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        if ctrlc.has_occurred() {
            interrupt::exit_on_sigint();
        }
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, status, self.command)
        }
    }
}

