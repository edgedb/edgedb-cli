use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::future::{Future, pending};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_process::{Command, Stdio, ExitStatus, Output};
use async_std::io::prelude::{BufReadExt};
use async_std::io::{self, Read, ReadExt, BufReader, WriteExt};
use async_std::prelude::{FutureExt, StreamExt};
use async_std::task;
use colorful::{Colorful, Color};
use serde::de::DeserializeOwned;

use crate::interrupt;


fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}


pub struct Native {
    command: Command,
    stop_process: Option<Box<dyn Fn() -> Command>>,
    marker: Cow<'static, str>,
    description: Cow<'static, str>,
    proxy: bool,
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

enum EnvVal {
    Propagate,
    Value(OsString),
}

#[cfg(unix)]
pub fn exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub fn exists(pid: u32) -> bool {
    use std::ptr::null_mut;
    use winapi::um::processthreadsapi::{OpenProcess};
    use winapi::um::winnt::{PROCESS_QUERY_INFORMATION};
    use winapi::um::handleapi::CloseHandle;

    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid) };
    if handle == null_mut() {
        // MSDN doesn't describe what is proper error here :(
        return false;
    }
    unsafe { CloseHandle(handle) };
    return true;
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
            proxy: clicolors_control::colors_enabled(),
            stop_process: None,
        }
    }
    pub fn no_proxy(&mut self) -> &mut Self {
        self.proxy = false;
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
    pub fn env_default(&mut self,
        name: impl AsRef<OsStr> + Into<OsString>,
        default: impl Into<OsString>)
        -> &mut Self
    {
        if env::var_os(name.as_ref()).is_none() {
            self.command.env(name.into(), default.into());
        } // otherwise it's normally propagated
        self
    }

    pub fn command_line(&self) -> impl fmt::Debug + '_ {
        &self.command
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let output = task::block_on(self._run(false, false))?;
        if output.status.success() {
            Ok(())
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, output.status, self.command);
        }
    }
    pub fn run_and_exit(&mut self) -> anyhow::Result<()> {
        let output = task::block_on(self._run(false, false))?;
        if let Some(code) = output.status.code() {
            exit(code);
        } else {
            anyhow::bail!("process {} (command-line: {:?}) failed: {}",
                self.description, self.command, output.status)
        }
    }
    pub fn run_or_stderr(&mut self)
        -> anyhow::Result<Result<(), (ExitStatus, String)>>
    {
        let output = task::block_on(self._run(false, true))?;
        if output.status.success() {
            Ok(Ok(()))
        } else {
            let data = String::from_utf8(output.stderr)
            .with_context(|| format!(
                "cannot decode error output of {} (command-line: {:?})",
                self.description, self.command,
            ))?;
            Ok(Err((output.status, data)))
        }
    }
    pub fn get_json_or_stderr<T>(&mut self)
        -> anyhow::Result<Result<T, String>>
        where T: DeserializeOwned,
    {
        let output = task::block_on(self._run(true, true))?;
        if output.status.success() {
            let value = serde_json::from_slice(&output.stdout[..])
                .with_context(|| format!(
                    "cannot decode output of {} (command-line: {:?})",
                    self.description, self.command,
                ))?;
            Ok(Ok(value))
        } else {
            let data = String::from_utf8(output.stderr)
            .with_context(|| format!(
                "cannot decode error output of {} (command-line: {:?})",
                self.description, self.command,
            ))?;
            Ok(Err(data))
        }
    }
    pub fn get_stdout_text(&mut self) -> anyhow::Result<String> {
        let output = task::block_on(self._run(true, false))?;
        if output.status.success() {
            let text = String::from_utf8(output.stdout)
                .with_context(|| format!(
                    "{} produced invalid utf-8 (command-line: {:?})",
                    self.description, self.command))?;
            Ok(text)
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, output.status, self.command);
        }
    }
    pub fn get_output(&mut self) -> anyhow::Result<Output> {
        task::block_on(self._run(true, true))
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

    async fn _run(&mut self, capture_out: bool, capture_err: bool)
        -> anyhow::Result<Output>
    {
        let term = interrupt::Interrupt::term();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        log::info!("Running {}: {:?}", self.description, self.command);
        if capture_out || self.proxy {
            self.command.stdout(Stdio::piped());
        }
        if capture_err || self.proxy {
            self.command.stderr(Stdio::piped());
        }
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id();

        let mark = &self.marker;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let child_result = child.status()
            .race(stdout_loop(mark, out, capture_out.then(|| &mut stdout)))
            .race(stdout_loop(mark, err, capture_err.then(|| &mut stderr)))
            .race(self.signal_loop(pid, &term))
            .await;
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                self.description, self.command))?;
        log::debug!("Result of {}: {}", self.description, status);
        Ok(Output { status, stdout, stderr })
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
            .race(self.signal_loop(pid, &term)).await;
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
        if self.proxy {
            self.command.stdout(Stdio::piped());
            self.command.stderr(Stdio::piped());
        }
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let pid = child.id();
        let result = self.run_and_kill(child, f)
            .race(stdout_loop(&self.marker, out, None))
            .race(stdout_loop(&self.marker, err, None))
            .race(self.signal_loop(pid, &term))
            .await;
        term.exit_if_occurred();
        return result;
    }

    async fn _feed(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let term = interrupt::Interrupt::term();
        log::info!("Running {}: {:?}", self.description, self.command);
        self.command.stdin(Stdio::piped());
        if self.proxy {
            self.command.stdout(Stdio::piped());
            self.command.stderr(Stdio::piped());
        }
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id();
        let inp = child.stdin.take().unwrap();
        let out = child.stdout.take();
        let err = child.stderr.take();
        let child_result = child.status()
            .race(feed_data(inp, data))
            .race(stdout_loop(&self.marker, out, None))
            .race(stdout_loop(&self.marker, err, None))
            .race(self.signal_loop(pid, &term))
            .await;
        term.exit_if_occurred();
        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                self.description, self.command))?;
        log::debug!("Result of {}: {}", self.description, status);
        if status.success() {
            Ok(())
        } else {
            anyhow::bail!("{} failed: {} (command-line: {:?})",
                          self.description, status, self.command);
        }
    }

    #[cfg(windows)]
    async fn signal_loop<Never>(&self, _: u32, _: &interrupt::Interrupt)
        -> Never
    {
        // on windows Ctrl+C signals are propagated automatically and no other
        // signals are supported, so there is nothing to do here
        wait_forever().await
    }

    #[cfg(unix)]
    async fn signal_loop<Never>(&self, pid: u32, intr: &interrupt::Interrupt)
        -> Never
    {
        use async_std::future::timeout;
        use signal_hook::consts::signal::{SIGTERM, SIGKILL};
        use std::time::Duration;

        let sig = intr.wait().await;
        match sig {
            interrupt::Signal::Interrupt => {
                log::warn!("Got interrupt. Waiting for \
                    the {} process to exit.", self.description);
            }
            interrupt::Signal::Hup => {
                log::warn!("Got HUP signal. Waiting for \
                    the {} process to exit.", self.description);
            }
            interrupt::Signal::Term => {
                log::warn!("Got TERM signal. Propagating to {}...",
                    self.description);
                if self.try_stop_process().await.is_err() {
                    if unsafe { libc::kill(pid as i32, SIGTERM) } != 0 {
                        log::debug!("Error stopping {}: {}",
                            self.description, io::Error::last_os_error());
                    }
                }
            }
        };
        timeout(Duration::from_secs(10), pending::<()>()).await.ok();
        log::warn!("Process {} did not stop in 10 seconds, forcing...",
            self.description);
        if self.try_stop_process().await.is_err() {
            unsafe { libc::kill(pid as i32, SIGKILL) };
        }
        wait_forever().await
    }

    async fn try_stop_process(&self) -> Result<(), ()> {
        if let Some(stop_fn) = &self.stop_process {
            let mut stop_cmd = stop_fn();
            log::debug!("Running {:?} to stop {}", stop_cmd, self.description);
            match stop_cmd.status().await {
                Ok(s) if s.success() => Ok(()),
                Ok(s) => {
                    log::debug!("Error signalling to {}: {:?}: {}",
                        self.description, stop_cmd, s);
                    // This probably means "container is already stopped" so
                    // we don't want to kill original docker process. That
                    // maybe doing `--rm` cleanup at the moment
                    Ok(())
                }
                Err(e) => {
                    log::warn!("Error running {:?}: {}", stop_cmd, e);
                    Err(())
                }
            }
        } else {
            Err(())
        }
    }

    async fn run_and_kill<T>(&self, mut child: async_process::Child,
        f: impl Future<Output=anyhow::Result<T>>)
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
                    self.description, self.command))
                .and_then(|status| {
                    log::debug!("Result of {} (background): {}",
                        self.description, status);
                    anyhow::bail!(
                        "{} exited prematurely: {} (command-line: {:?})",
                        self.description, status, self.command);
                })
            }
            Ok(result) => {
                log::debug!("Stopping {}", self.description);
                if self.try_stop_process().await.is_ok() {
                    let status = child.status().await.with_context(|| format!(
                        "failed to get status of {} (command-line: {:?})",
                        self.description, self.command))?;
                    log::debug!("Result of {} (background): {}",
                        self.description, status);
                } else {
                    if cfg!(windows) {
                        if let Err(e) = child.kill() {
                            log::error!("Error stopping {}: {}",
                                self.description, e);
                        }
                        let status = child.status().await
                            .with_context(|| format!(
                            "failed to get status of {} (command-line: {:?})",
                            self.description, self.command))?;
                        log::debug!("Result of {} (background): {}",
                            self.description, status);
                    }
                    #[cfg(unix)] {
                        let pid = child.id();
                        let status = child.status()
                            .race(kill_child(pid, &self.description))
                            .await
                            .with_context(|| format!(
                                "failed to get status of {} \
                                (command-line: {:?})",
                                self.description, self.command))?;
                        log::debug!("Result of {} (background): {}",
                            self.description, status);
                    }
                    #[cfg(not(any(windows, unix)))]
                    compile_error!("unknown platform");
                }
                result
            }
        }
    }
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
    fn make_native(&self, interactive: bool) -> Native {
        let name = format!("edgedb_{}_{}", std::process::id(), timestamp());
        let docker = self.docker_cmd.clone();

        let mut cmd = Native::new(self.description.clone(), "docker", &docker);
        cmd.arg("run");
        cmd.arg("--rm");
        cmd.arg("--name").arg(&name);
        if interactive {
            cmd.arg("--interactive");
        }
        for (key, val) in &self.env {
            match val {
                EnvVal::Propagate => {
                    cmd.arg("--env").arg(key);
                }
                EnvVal::Value(val) => {
                    let mut arg = key.clone();
                    arg.push("=");
                    arg.push(val);
                    cmd.arg("--env").arg(arg);
                }
            }
        }

        for mnt in &self.mounts {
            cmd.arg("--mount").arg(mnt);
        }
        if self.set_user {
            cmd.arg("--user=999:999");
        }
        for port in &self.expose_ports {
            cmd.arg(format!("--publish={0}:{0}", port));
        }
        cmd.arg(&self.image[..]);
        cmd.arg(&self.cmd[..]);
        for arg in &self.arguments {
            cmd.arg(arg);
        }
        cmd.stop_process = Some(Box::new(move || {
            let mut cmd = Command::new(&docker);
            cmd.arg("stop");
            cmd.arg(&name);
            return cmd;
        }));
        return cmd;
    }
    pub fn feed(&self, data: impl AsRef<[u8]>) -> anyhow::Result<()> {
        self.make_native(true).feed(data)
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        self.make_native(false).run()
    }
    pub fn get_stdout_text(&mut self) -> anyhow::Result<String> {
        self.make_native(false).get_stdout_text()
    }
    pub fn get_output(&mut self) -> anyhow::Result<Output> {
        self.make_native(false).get_output()
    }
    pub fn background_for<T>(&mut self,
        f: impl Future<Output=anyhow::Result<T>>)
        -> anyhow::Result<T>
    {
        self.make_native(false).background_for(f)
    }
}

async fn stdout_loop<Never>(marker: &str, pipe: Option<impl Read+Unpin>,
    capture_buffer: Option<&mut Vec<u8>>)
    -> Never
{
    match (pipe, capture_buffer) {
        (Some(mut pipe), Some(buffer)) => {
            pipe.read_to_end(buffer).await.map_err(|e| {
                log::info!("Cannot read command's output: {}", e);
            }).ok();
        }
        (Some(pipe), None) => {
            let buf = BufReader::new(pipe);
            let mut lines = buf.lines();
            while let Some(Ok(line)) = lines.next().await {
                io::stderr().write_all(
                    format!("[{}] {}\n", marker, line).color(Color::Grey37)
                    .to_string().as_bytes()
                ).await.ok();
            }
        }
        (None, Some(_)) => unreachable!(),
        (None, None) => {}
    }
    wait_forever().await
}

#[cfg(unix)]
async fn kill_child<Never>(pid: u32, description: &str) -> Never {
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

async fn feed_data<Never>(mut inp: impl io::Write + Unpin, data: &[u8])
    -> Never
{
    // Don't care if input is not written,
    // rely on command status
    inp.write_all(data).await.ok();
    drop(inp);
    wait_forever().await
}

async fn wait_forever() -> ! {
    pending::<()>().await;
    unreachable!();
}



