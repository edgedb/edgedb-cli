use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::future::{Future, pending};
use std::path::{Path, PathBuf};

use anyhow::Context;
use async_process::{Command, Stdio, ExitStatus, Output};
use async_std::io::prelude::{BufReadExt};
use async_std::io::{self, Read, BufReader, WriteExt};
use async_std::prelude::{FutureExt, StreamExt};
use async_std::task;
use colorful::{Colorful, Color};

use crate::interrupt;


pub struct Native {
    command: Command,
    marker: Cow<'static, str>,
    description: Cow<'static, str>,
    capture: bool,
}

enum EnvVal {
    Propagate,
    Value(OsString),
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
            capture: clicolors_control::colors_enabled(),
        }
    }
    pub fn no_capture(&mut self) -> &mut Self {
        self.capture = false;
        self
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
    pub fn background_for<T>(&mut self,
        f: impl Future<Output=anyhow::Result<T>>)
        -> anyhow::Result<T>
    {
        task::block_on(self._background(f))
    }
    pub fn feed(&mut self, data: impl AsRef<[u8]>) -> anyhow::Result<()> {
        task::block_on(self._feed(data.as_ref()))
    }
    pub fn status(&mut self) -> anyhow::Result<ExitStatus> {
        task::block_on(self._status())
    }

    async fn _run(&mut self) -> anyhow::Result<()> {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        let child_result = if self.capture {
            self.command.stdout(Stdio::piped());
            self.command.stderr(Stdio::piped());
            let mut child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let pid = child.id();
            let out = child.stdout.take().unwrap();
            let err = child.stderr.take().unwrap();
            child.status()
                .race(async { stdout_loop(&self.marker, out).await })
                .race(async { stdout_loop(&self.marker, err).await })
                .race(async {
                    process_loop(pid, &term, &self.description).await
                })
                .await
        } else {
            let mut child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let pid = child.id();
            child.status()
                .race(async {
                    process_loop(pid, &term, &self.description).await
                }).await
        };
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                self.description, self.command))?;
        if status.success() {
            log::debug!("Result of {}: {}", self.description, status);
            Ok(())
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, status, self.command);
        }
    }

    async fn _status(&mut self) -> anyhow::Result<ExitStatus> {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        self.command.stdout(Stdio::null());
        self.command.stderr(Stdio::null());
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id();
        let child_result = child.status()
            .race(async {
                process_loop(pid, &term, &self.description).await
            }).await;
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                self.description, self.command))?;
        log::debug!("Result of {}: {}", self.description, status);
        Ok(status)
    }

    async fn _background<T>(&mut self,
        f: impl Future<Output=anyhow::Result<T>>)
        -> anyhow::Result<T>
    {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        let result = if self.capture {
            self.command.stdout(Stdio::piped());
            self.command.stderr(Stdio::piped());
            let mut child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let out = child.stdout.take().unwrap();
            let err = child.stderr.take().unwrap();
            let pid = child.id();
            run_and_kill(child, f, &self.description, &self.command)
                .race(async { stdout_loop(&self.marker, out).await })
                .race(async { stdout_loop(&self.marker, err).await })
                .race(async {
                    process_loop(pid, &term, &self.description).await
                })
                .await
        } else {
            let child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let pid = child.id();
            run_and_kill(child, f, &self.description, &self.command)
                .race(async {
                    process_loop(pid, &term, &self.description).await
                }).await
        };
        term.exit_if_occurred();
        result
    }

    async fn _feed(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        self.command.stdin(Stdio::piped());
        let child_result = if self.capture {
            self.command.stdout(Stdio::piped());
            self.command.stderr(Stdio::piped());
            let mut child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let pid = child.id();
            let mut inp = child.stdin.take().unwrap();
            let out = child.stdout.take().unwrap();
            let err = child.stderr.take().unwrap();
            child.status()
                .race(async {
                    // Don't care if input is not written,
                    // rely on command status
                    inp.write_all(data).await.ok();
                    drop(inp);
                    wait_forever().await
                })
                .race(async { stdout_loop(&self.marker, out).await })
                .race(async { stdout_loop(&self.marker, err).await })
                .race(async {
                    process_loop(pid, &term, &self.description).await
                })
                .await
        } else {
            let mut child = self.command.spawn()
                .with_context(|| format!(
                    "{} failed to start (command-line: {:?})",
                    self.description, self.command))?;
            let pid = child.id();
            let mut inp = child.stdin.take().unwrap();
            child.status()
                .race(async {
                    // Don't care if input is not written,
                    // rely on command status
                    inp.write_all(data).await.ok();
                    wait_forever().await
                })
                .race(async {
                    process_loop(pid, &term, &self.description).await
                }).await
        };
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
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

pub struct Docker {
    docker_cmd: PathBuf,
    description: Cow<'static, str>,
    env: BTreeMap<OsString, EnvVal>,
    mounts: Vec<String>,
    arguments: Vec<OsString>,
    set_user: bool,
    expose_ports: Vec<u16>,
    image: Cow<'static, str>,
    cmd: Cow<'static, str>,
}

impl Docker {
    pub fn new(description: impl Into<Cow<'static, str>>,
        docker_cmd: impl AsRef<Path>,
        image: impl Into<Cow<'static, str>>,
        cmd: impl Into<Cow<'static, str>>)
        -> Docker
    {
        Docker {
            docker_cmd: docker_cmd.as_ref().into(),
            description: description.into(),
            env: BTreeMap::new(),
            mounts: Vec::new(),
            arguments: Vec::new(),
            set_user: true,
            expose_ports: Vec::new(),
            image: image.into(),
            cmd: cmd.into(),
        }
    }
    pub fn env(&mut self, name: impl Into<OsString>,
        value: impl Into<OsString>)
        -> &mut Self
    {
        self.env.insert(name.into(), EnvVal::Value(value.into()));
        self
    }
    pub fn env_default(&mut self,
        name: impl AsRef<OsStr> + Into<OsString>,
        default: impl Into<OsString>)
        -> &mut Self
    {
        if env::var_os(name.as_ref()).is_some() {
            self.env.insert(name.into(), EnvVal::Propagate);
        } else {
            self.env.insert(name.into(), EnvVal::Value(default.into()));
        }
        self
    }
    pub fn expose_port(&mut self, port: u16) -> &mut Self {
        self.expose_ports.push(port);
        self
    }
    pub fn as_root(&mut self) -> &mut Self {
        self.set_user = false;
        self
    }
    pub fn mount(&mut self, source: impl AsRef<str>, target: impl AsRef<str>)
        -> &mut Self
    {
        assert!(!source.as_ref().contains(","));
        assert!(!target.as_ref().contains(","));
        self.mounts.push(format!("source={},target={}",
            source.as_ref(), target.as_ref()));
        self
    }
    pub fn arg(&mut self, val: impl Into<OsString>) -> &mut Self {
        self.arguments.push(val.into());
        self
    }
    pub fn feed(&self, data: impl AsRef<[u8]>) -> anyhow::Result<()> {
        todo!();
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        todo!();
    }
    pub fn get_stdout_text(&mut self) -> anyhow::Result<String> {
        todo!();
    }
    pub fn get_output(&mut self) -> anyhow::Result<Output> {
        todo!();
    }
    pub fn background_for<T>(&mut self,
        f: impl Future<Output=anyhow::Result<T>>)
        -> anyhow::Result<T>
    {
        todo!();
    }
}

#[cfg(windows)]
async fn process_loop(_: u32, _: &interrupt::Interrupt, _: &str) -> !
{
    // on windows Ctrl+C signals are propagated automatically and no other
    // signals are supported, so there is nothing to do here
    wait_forever().await
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
    wait_forever().await
}

async fn stdout_loop(marker: &str, pipe: impl Read+Unpin) -> ! {
    let buf = BufReader::new(pipe);
    let mut lines = buf.lines();
    while let Some(Ok(line)) = lines.next().await {
        io::stderr().write_all(
            format!("[{}] {}\n", marker, line).color(Color::Grey37)
            .to_string().as_bytes()
        ).await.ok();
    }
    wait_forever().await
}

async fn run_and_kill<T>(mut child: async_process::Child,
    f: impl Future<Output=anyhow::Result<T>>,
    description: &str, cmd: &Command)
    -> anyhow::Result<T>
{
    let result = async { Err(child.status().await) }
        .race(async { Ok(f.await) })
        .await;
    match result {
        Err(process_result) => {
            process_result
            .with_context(|| format!(
                "failed to wait for {} (command-line: {:?})",
                description, cmd))
            .and_then(|status| {
                anyhow::bail!("{} exited prematurely: {} (command-line: {:?})",
                              description, status, cmd);
            })
        }
        #[cfg(windows)]
        Ok(result) => {
            log::debug!("Stopping {}", description);
            if let Err(e) = child.kill() {
                log::error!("Error stopping {}: {}", description, e);
            }
            child.status().await.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                description, cmd))?;
            result
        }
        #[cfg(unix)]
        Ok(result) => {
            let pid = child.id();
            child.status()
                .race(async { kill_child(pid, description).await })
                .await
                .with_context(|| format!(
                    "failed to get status of {} (command-line: {:?})",
                    description, cmd))?;
            result
        }
    }
}

#[cfg(unix)]
async fn kill_child(pid: u32, description: &str) -> ! {
    use signal_hook::consts::signal::{SIGTERM, SIGKILL};
    use async_std::future::timeout;
    use std::time::Duration;

    log::debug!("Stopping {}", description);
    if unsafe { libc::kill(pid as i32, SIGTERM) } != 0 {
        log::error!("Error stopping {}: {}", description,
            io::Error::last_os_error());
    }
    timeout(Duration::from_secs(10), pending::<()>()).await.ok();
    log::warn!("Process {} takes too long to complete. Forcing...",
        description);
    if unsafe { libc::kill(pid as i32, SIGKILL) } != 0 {
        log::debug!("Error stopping {}: {}", description,
            io::Error::last_os_error());
    }
    wait_forever().await
}

async fn wait_forever() -> ! {
    pending::<()>().await;
    unreachable!();
}
