use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::future::{Future, pending};
use std::path::{Path, PathBuf};
use std::process::exit;

use anyhow::Context;
use async_process::{Command, Stdio, ExitStatus, Output};
use async_std::io::prelude::{BufReadExt};
use async_std::io::{self, Read, ReadExt, BufReader, WriteExt};
use async_std::prelude::{FutureExt, StreamExt};
use async_std::task;
use colorful::{Colorful, Color};
use once_cell::sync::Lazy;

use crate::interrupt;
use crate::platform::tmp_file_path;

#[cfg(unix)]
static HAS_UTF8_LOCALE: Lazy<bool> = Lazy::new(|| {
    use std::ptr::null_mut;
    use std::ffi::CString;

    let utf8_term = ["LANG", "LC_ALL", "LC_MESSAGES"].iter().any(|n| {
        env::var(n)
            .map(|v| v.contains("utf8") || v.contains("UTF-8"))
            .unwrap_or(false)
    });
    if utf8_term {
        unsafe {
            let c_utf8 = CString::new("C.UTF-8").unwrap();
            let loc = libc::newlocale(libc::LC_ALL,
                                      c_utf8.as_ptr(), null_mut());
            if loc != null_mut() {
                libc::freelocale(loc);
                log::debug!("UTF-8 locale is enabled");
                return true;
            } else {
                log::debug!("Cannot load C.UTF-8");
                return false;
            }
        }
    } else {
        log::debug!("UTF-8 not enabled (non-utf-8 locale)");
        return false;
    }
});


pub struct Native {
    command: Command,
    program: OsString,
    args: Vec<OsString>,
    envs: HashMap<OsString, Option<OsString>>,
    stop_process: Option<Box<dyn Fn() -> Command>>,
    marker: Cow<'static, str>,
    description: Cow<'static, str>,
    proxy: bool,
    pid_file: Option<PathBuf>,
}

#[cfg(unix)]
pub fn term(pid: u32) -> anyhow::Result<()>{
    use signal_hook::consts::signal::{SIGTERM};

    if unsafe { libc::kill(pid as i32, SIGTERM) } != 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("cannot kill {}", pid))?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn term(pid: u32) -> anyhow::Result<()>{
    use std::ptr::null_mut;
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::winnt::{PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE};
    use winapi::um::handleapi::CloseHandle;

    let handle = unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_TERMINATE, 0, pid)
    };
    if handle == null_mut() {
        // MSDN doesn't describe what is proper error here :(
        anyhow::bail!("process could not be found or cannot be killed");
    }
    unsafe { TerminateProcess(handle, 1) };
    unsafe { CloseHandle(handle) };
    Ok(())
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

pub trait IntoArg {
    fn add_arg(self, process: &mut Native);
}

impl IntoArg for &String {
    fn add_arg(self, process: &mut Native) {
        process.arg(self);
    }
}

impl IntoArg for &u16 {
    fn add_arg(self, process: &mut Native) {
        process.arg(self.to_string());
    }
}

impl IntoArg for &usize {
    fn add_arg(self, process: &mut Native) {
        process.arg(self.to_string());
    }
}

pub trait IntoArgs {
    fn add_args(self, process: &mut Native);
}

impl<I: IntoArg, T: IntoIterator<Item=I>> IntoArgs for T {
    fn add_args(self, process: &mut Native) {
        for item in self.into_iter() {
            item.add_arg(process);
        }
    }
}

impl Native {
    pub fn new(description: impl Into<Cow<'static, str>>,
        marker: impl Into<Cow<'static, str>>,
        cmd: impl AsRef<Path>)
        -> Native
    {
        let mut me = Native {
            description: description.into(),
            marker: marker.into(),
            command: Command::new(cmd.as_ref()),
            program: cmd.as_ref().as_os_str().to_os_string(),
            args: vec![cmd.as_ref().as_os_str().to_os_string()],
            envs: HashMap::new(),
            proxy: clicolors_control::colors_enabled(),
            stop_process: None,
            pid_file: None,
        };
        #[cfg(unix)] {
            if *HAS_UTF8_LOCALE {
                me.env("LANG", "C.UTF-8");
                me.env("LC_ALL", "C.UTF-8");
            } else {
                me.env("LANG", "C");
                me.env("LC_ALL", "C");
            }
        }
        if cfg!(target_os="macos") {
            me.env("LC_CTYPE", "UTF-8");
        }
        return me;
    }
    pub fn no_proxy(&mut self) -> &mut Self {
        self.proxy = false;
        self
    }

    pub fn pid_file(&mut self, path: &Path) -> &mut Self {
        self.pid_file = Some(path.to_path_buf());
        self
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub fn log_file(&mut self, path: &Path) -> anyhow::Result<&mut Self>
    {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(&dir)?;
        }
        let file = fs::OpenOptions::new()
            .write(true).append(true).create(true)
            .open(&path)
            .with_context(|| format!("cannot open log file {:?}", path))?;
        self.command.stdout(file.try_clone().context("cannot clone file")?);
        self.command.stderr(file);
        self.proxy = false;
        Ok(self)
    }

    pub fn arg(&mut self, val: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(val.as_ref());
        self.args.push(val.as_ref().to_os_string());
        self
    }

    pub fn args(&mut self, val: impl IntoArgs) -> &mut Self {
        val.add_args(self);
        self
    }

    pub fn env(&mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>)
        -> &mut Self
    {
        self.envs.insert(
            key.as_ref().to_os_string(),
            Some(val.as_ref().to_os_string()),
        );
        self.command.env(key, val);
        self
    }
    pub fn env_default(&mut self,
        name: impl AsRef<OsStr> + Into<OsString>,
        default: impl Into<OsString>)
        -> &mut Self
    {
        if env::var_os(name.as_ref()).is_none() {
            self.env(name.into(), default.into());
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

    /// EOS for stdout here means that process is safefully started.
    /// We return stdout as text just because we can and we might find a
    /// useful case for this later.
    pub fn daemonize_with_stdout(&mut self) -> anyhow::Result<Vec<u8>> {
        task::block_on(self._daemonize())
    }

    #[allow(dead_code)]
    pub fn background_for<T>(&mut self,
        f: impl Future<Output=anyhow::Result<T>>)
        -> anyhow::Result<T>
    {
        task::block_on(self._background(f))
    }
    #[allow(dead_code)]
    pub fn feed(&mut self, data: impl AsRef<[u8]>) -> anyhow::Result<()> {
        task::block_on(self._feed(data.as_ref()))
    }
    /// Redirects stdout+stderr into /dev/null and returns status
    pub fn status_only(&mut self) -> anyhow::Result<ExitStatus> {
        task::block_on(self._status())
    }
    pub fn status(&mut self) -> anyhow::Result<ExitStatus> {
        task::block_on(self._run(false, false)).map(|out| out.status)
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
        write_pid_file(&self.pid_file, pid);

        let mark = &self.marker;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let ((child_result, _), _) = child.status()
            .join(stdout_loop(mark, out, capture_out.then(|| &mut stdout)))
            .join(stdout_loop(mark, err, capture_err.then(|| &mut stderr)))
            .race(self.signal_loop(pid, &term))
            .await;

        remove_pid_file(&self.pid_file);
        term.err_if_occurred()?;

        let status = child_result.with_context(|| format!(
                "failed to get status of {} (command-line: {:?})",
                self.description, self.command))?;
        log::debug!("Result of {}: {}", self.description, status);
        Ok(Output { status, stdout, stderr })
    }

    async fn _daemonize(&mut self) -> anyhow::Result<Vec<u8>> {
        let term = interrupt::Interrupt::term();
        let mut stdout = Vec::new();
        log::info!("Daemonizing {}: {:?}", self.description, self.command);
        self.command.stdout(Stdio::piped());
        let mut child = self.command.spawn()
            .with_context(|| format!(
                "{} failed to start (command-line: {:?})",
                self.description, self.command))?;
        let pid = child.id();
        write_pid_file(&self.pid_file, pid);

        let mark = &self.marker;
        let out = child.stdout.take();
        let mut res = async { Err(child.status().await) }
            .race(async { Ok(stdout_loop(mark, out, Some(&mut stdout)).await) })
            .race(self.signal_loop(pid, &term))
            .await;

        remove_pid_file(&self.pid_file);
        term.err_if_occurred()?;

        if res.is_ok() {
            // After stdout is finished check that process is still alive.
            // This way we figure out whether stdout was intentionally closed
            // or because process is shut down
            if let Some(exit) = child.try_status().transpose() {
                res = Err(exit);
            }
        }

        res.map_err(|res| match res {
            Ok(status) => anyhow::anyhow!(
                "failed to run {} (command-line: {:?}): {}",
                self.description, self.command, status),
            Err(e) => anyhow::anyhow!(
                "failed to run {} (command-line: {:?}): {}",
                self.description, self.command, e),
        })?;

        log::debug!("Process {} daemonized with output: {:?}",
                    self.description, stdout);
        Ok(stdout)
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
        write_pid_file(&self.pid_file, pid);

        let child_result = child.status()
            .race(self.signal_loop(pid, &term)).await;

        remove_pid_file(&self.pid_file);
        term.err_if_occurred()?;

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
        write_pid_file(&self.pid_file, pid);

        let ((result, _), _) = self.run_and_kill(child, f)
            .join(stdout_loop(&self.marker, out, None))
            .join(stdout_loop(&self.marker, err, None))
            .race(self.signal_loop(pid, &term))
            .await;

        remove_pid_file(&self.pid_file);
        term.err_if_occurred()?;

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
        write_pid_file(&self.pid_file, pid);

        let inp = child.stdin.take().unwrap();
        let out = child.stdout.take();
        let err = child.stderr.take();
        let ((child_result, _), _) = child.status()
            .race(feed_data(inp, data))
            .join(stdout_loop(&self.marker, out, None))
            .join(stdout_loop(&self.marker, err, None))
            .race(self.signal_loop(pid, &term))
            .await;

        remove_pid_file(&self.pid_file);
        term.err_if_occurred()?;

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
    pub fn stop_process<F>(&mut self, f: F) -> &mut Self
        where F: Fn() -> Command + 'static
    {
        self.stop_process = Some(Box::new(f));
        self
    }
    /// Replace current process with this one instead off spawning
    #[cfg(unix)]
    pub fn exec_replacing_self(&self)
        -> anyhow::Result<std::convert::Infallible>
    {
        use nix::unistd::execve;
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        log::debug!("Replacing CLI with {:?}", self.command);

        fn env_pair(key: &OsStr, val: &OsStr)
            -> anyhow::Result<CString>
        {
            let mut cstr = Vec::with_capacity(key.len() + val.len() + 2);
            cstr.extend(key.as_bytes());
            cstr.push(b'=');
            cstr.extend(val.as_bytes());
            return Ok(CString::new(cstr)?);
        }

        let mut env = Vec::new();
        for (key, val) in &self.envs {
            if let Some(val) = val {
                env.push(env_pair(key, val)?);
            }
        }
        for (key, val) in env::vars_os() {
            if !self.envs.contains_key(&key) {
                env.push(env_pair(&key, &val)?);
            }
        }

        execve(
            &CString::new(self.program.as_bytes())?,
            &self.args.iter()
                .map(|arg| CString::new(arg.as_bytes()))
                .collect::<Result<Vec<_>, _>>()?,
            &env,
        )?;
        unreachable!();
    }
    /// Replace current process with this one instead off spawning
    #[cfg(not(unix))]
    pub fn exec_replacing_self(&self)
        -> anyhow::Result<std::convert::Infallible>
    {
        unimplemented!();
    }
}


async fn stdout_loop(marker: &str, pipe: Option<impl Read+Unpin>,
    capture_buffer: Option<&mut Vec<u8>>)
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
                if cfg!(windows) {
                    io::stderr().write_all(
                        format!("[{}] {}\r\n", marker, line)
                        .color(Color::Grey37)
                        .to_string().as_bytes()
                    ).await.ok();
                } else {
                    io::stderr().write_all(
                        format!("[{}] {}\n", marker, line)
                        .color(Color::Grey37)
                        .to_string().as_bytes()
                    ).await.ok();
                }
            }
        }
        (None, Some(_)) => unreachable!(),
        (None, None) => {}
    }
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

fn write_pid_file(path: &Option<PathBuf>, pid: u32) {
    log::debug!("Writing pid file {:?} (pid: {})", path, pid);
    if let Some(path) = path {
        _write_pid_file(path, pid).map_err(|e| {
            log::error!("Cannot write pid file {:?}: {:#}", path, e);
        }).ok();
    }
}

fn remove_pid_file(path: &Option<PathBuf>) {
    if let Some(path) = path {
        fs::remove_file(&path).map_err(|e| {
            log::error!("Cannot remove pid file {:?}: {:#}", path, e);
        }).ok();
    }
}

fn _write_pid_file(path: &Path, pid: u32) -> anyhow::Result<()> {
    let tmp_path = tmp_file_path(&path);
    fs::remove_file(&tmp_path).ok();
    fs::write(&tmp_path, pid.to_string().as_bytes())?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}
