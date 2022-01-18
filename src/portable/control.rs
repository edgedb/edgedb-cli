use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::env;
use std::fs;

use anyhow::Context;
use fn_error_context::context;

use crate::credentials;
use crate::bug;
use crate::hint::HintExt;
use crate::portable::local::{InstanceInfo, runstate_dir, open_lock, lock_file};
use crate::portable::options::{Start, Stop, Restart, Logs};
use crate::portable::ver;
use crate::portable::{windows, linux, macos};
use crate::process;
use crate::platform::current_exe;


fn supervisor_start(inst: &InstanceInfo) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::start_service(&inst.name)
    } else if cfg!(target_os="macos") {
        macos::start_service(inst)
    } else if cfg!(target_os="linux") {
        linux::start_service(&inst.name)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

fn daemon_start(instance: &str) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::daemon_start(instance)
    } else {
        let lock = open_lock(instance)?;
        if lock.try_read().is_err() {  // properly running
            log::info!("Instance {:?} is already running", instance);
            return Ok(())
        }
        process::Native::new("edgedb cli", "edgedb-cli", &current_exe()?)
            .arg("instance")
            .arg("start")
            .arg(instance)
            .arg("--managed-by=edgedb-cli")
            .daemonize_with_stdout()?;
        Ok(())
    }
}

pub fn do_start(inst: &InstanceInfo) -> anyhow::Result<()> {
    let cred_path = credentials::path(&inst.name)?;
    if !cred_path.exists() {
        log::warn!("No corresponding credentials file {:?} exists. \
                    Use `edgedb instance reset-password {}` to create one.",
                    cred_path, inst.name);
    }
    if detect_supervisor(&inst.name) {
        supervisor_start(inst)
    } else {
        daemon_start(&inst.name)
    }
}

pub fn get_server_cmd(inst: &InstanceInfo) -> anyhow::Result<process::Native> {
    if cfg!(windows) {
        windows::server_cmd(&inst.name)
    } else if cfg!(target_os="macos") {
        macos::server_cmd(inst)
    } else if cfg!(target_os="linux") {
        linux::server_cmd(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn ensure_runstate_dir(name: &str) -> anyhow::Result<PathBuf> {
    let runstate_dir = runstate_dir(name)?;
    match fs::create_dir_all(&runstate_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && cfg!(unix)
        => {
            return Err(e)
                .context(format!(
                    "failed to create runstate dir {:?}", runstate_dir))
                .hint("This may mean that `XDG_RUNTIME_DIR` \
                    is inherited from another user's environment. \
                    Run `unset XDG_RUNTIME_DIR` or use a better login-as-user \
                    tool (use `sudo` instead of `su`).")?;
        }
        Err(e) => {
            return Err(e)
                .context(format!(
                    "failed to create runstate dir {:?}", runstate_dir));
        }
    }
    Ok(runstate_dir)
}

#[context("cannot write lock metadata at {:?}", path)]
fn write_lock_info(path: &Path, lock: &mut fs::File,
                   marker: &Option<String>)
    -> anyhow::Result<()>
{
    use std::io::Write;

    lock.set_len(0)?;
    lock.write(marker.as_ref().map(|x| &x[..]).unwrap_or("user").as_bytes())?;
    Ok(())
}

pub fn detect_supervisor(name: &str) -> bool {
    if cfg!(windows) {
        false
    } else if cfg!(target_os="macos") {
        macos::detect_gui_session()
    } else if cfg!(target_os="linux") {
        linux::detect_systemd(name)
    } else {
        false
    }
}

#[cfg(unix)]
fn run_server_by_cli(meta: &InstanceInfo) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;
    use async_std::future::pending;
    use async_std::os::unix::net::UnixDatagram;
    use async_std::task;
    use crate::portable::local::log_file;

    unsafe { libc::setsid() };

    let pid_path = runstate_dir(&meta.name)?.join("edgedb.pid");
    let log_path = log_file(&meta.name)?;
    if let Some(dir) = log_path.parent() {
        fs_err::create_dir_all(&dir)?;
    }
    let log_file = fs_err::OpenOptions::new()
        .create(true).write(true).append(true)
        .open(&log_path)?;
    let null = fs_err::OpenOptions::new().write(true).open("/dev/null")?;
    let notify_socket = runstate_dir(&meta.name)?.join(".s.daemon");
    if notify_socket.exists() {
        fs_err::remove_file(&notify_socket)?;
    } if let Some(dir) = notify_socket.parent() {
        fs_err::create_dir_all(dir)?;
    }
    let sock = task::block_on(UnixDatagram::bind(&notify_socket))
        .context("cannot create notify socket")?;

    get_server_cmd(&meta)?
        .env("NOTIFY_SOCKET", &notify_socket)
        .pid_file(&pid_path)
        .log_file(&log_path)?
        .background_for(async {
            let mut buf = [0u8; 1024];
            while !matches!(sock.recv(&mut buf).await,
                           Ok(len) if &buf[..len] == b"READY=1")
            { };

            // Redirect stderr to log file, right before daemonizing.
            // So that all early errors are visible, but all later ones
            // (i.e. a message on term) do not clobber user's terminal.
            if unsafe { libc::dup2(log_file.as_raw_fd(), 2) } < 0 {
                return Err(io::Error::last_os_error())
                    .context("cannot close stdout")?;
            }
            drop(log_file);

            // Closing stdout to notify that daemon is successfully started.
            // Note: we can't just close the file descriptor as it will be
            // replaced with something unexpected on any next new file
            // descriptor creation. So we replace it with `/dev/null` (the
            // writing end of the original pipe is closed at this point).
            if unsafe { libc::dup2(null.as_raw_fd(), 1) } < 0 {
                return Err(io::Error::last_os_error())
                    .context("cannot close stdout")?;
            }
            drop(null);

            Ok(pending::<()>().await)
        })
}

#[cfg(windows)]
fn run_server_by_cli(_meta: &InstanceInfo) -> anyhow::Result<()> {
    anyhow::bail!("daemonizing is not yet supported for Windows");
}

pub fn start(options: &Start) -> anyhow::Result<()> {
    let meta = InstanceInfo::read(&options.name)?;
    ensure_runstate_dir(&meta.name)?;
    if options.foreground || options.managed_by.is_some() {
        let lock_path = lock_file(&meta.name)?;
        let mut lock = open_lock(&meta.name)?;
        let mut needs_restart = false;
        let try_write = lock.try_write();
        let lock = if let Ok(mut lock) = try_write {
            write_lock_info(&lock_path, &mut *lock, &options.managed_by)?;
            lock
        } else {
            drop(try_write);
            let locked_by = fs_err::read_to_string(&lock_path)
                .with_context(|| format!("cannot read lock file {:?}",
                                         lock_path))?;
            if options.managed_by.is_some() {
                log::warn!("Process is already running by {}. \
                            Waiting for that process to be stopped...",
                            locked_by.escape_default());
            } else if options.auto_restart {
                log::warn!("Process is already running by {}. \
                            Stopping...", locked_by.escape_default());
                needs_restart = true;
                do_stop(&options.name)
                    .context("cannot stop service")?;
            } else {
                anyhow::bail!("Process is already running by {}. \
                    Please stop the service manually or run \
                    with `--auto-restart` option.",
                    locked_by.escape_default());
            }
            let mut lock = lock.write()?;
            write_lock_info(&lock_path, &mut *lock, &options.managed_by)?;
            lock
        };
        if matches!(options.managed_by.as_deref(), Some("edgedb-cli")) {
            debug_assert!(!needs_restart);
            run_server_by_cli(&meta)
        } else {

            let res;
            if matches!(options.managed_by.as_deref(), Some("systemd")) &&
               env::var_os("NOTIFY_SOCKET").is_some() &&
               cfg!(target_os="linux")
            {
                res = linux::run_and_proxy_notify_socket(&meta);
            } else {
                res = get_server_cmd(&meta)?
                    .env_default("EDGEDB_SERVER_LOG_LEVEL", "info")
                    .no_proxy()
                    .run();
            }

            drop(lock);
            if needs_restart {
                log::warn!("Restarting service back into background...");
                do_start(&meta).map_err(|e| {
                    log::warn!("Error starting service: {}", e);
                }).ok();
            }
            Ok(res?)
        }
    } else {
        do_start(&meta)
    }
}

fn supervisor_stop(name: &str) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::stop_service(name)
    } else if cfg!(target_os="macos") {
        macos::stop_service(name)
    } else if cfg!(target_os="linux") {
        linux::stop_service(name)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn read_pid(instance: &str) -> anyhow::Result<Option<u32>> {
    let pid_path = runstate_dir(instance)?.join("edgedb.pid");
    match fs_err::read_to_string(&pid_path) {
        Ok(pid_str) => {
            let pid = pid_str.trim().parse().with_context(
                || format!("cannot parse pid file {:?}", pid_path))?;
            Ok(Some(pid))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(e) => {
            return Err(e)
                .context(format!("cannot read pid file {:?}", pid_path))?;
        }
    }
}

fn is_run_by_supervisor(lock: fd_lock::RwLock<fs::File>) -> bool {
    let mut buf = String::with_capacity(100);
    if lock.into_inner().read_to_string(&mut buf).is_err() {
        return false;
    }
    log::debug!("Service running by {:?}", buf);
    match &buf[..] {
        "systemd" if cfg!(target_os="linux") => true,
        "launchctl" if cfg!(target_os="macos") => true,
        _ => false,
    }
}

pub fn do_stop(name: &str) -> anyhow::Result<()> {
    let lock = open_lock(name)?;
    let supervisor = detect_supervisor(name);
    if lock.try_read().is_err() {  // properly running
        if supervisor && is_run_by_supervisor(lock) {
            supervisor_stop(name)
        } else {
            if let Some(pid) = read_pid(name)? {
                log::info!("Killing EdgeDB with pid {}", pid);
                process::term(pid)?;
                // wait for unlock
                let _ = open_lock(name)?
                    .read().context("cannot acquire read lock")?;
                Ok(())
            } else {
                return Err(bug::error("cannot find pid"));
            }
        }
    } else {  // probably not running
        if supervisor {
            supervisor_stop(name)
        } else {
            if let Some(pid) = read_pid(name)? {
                log::info!("Killing EdgeDB with pid {}", pid);
                process::term(pid)?;
                // wait for unlock
                let _ = open_lock(name)?.read()?;
            } // nothing to do
            Ok(())
        }
    }
}

pub fn stop(options: &Stop) -> anyhow::Result<()> {
    let meta = InstanceInfo::read(&options.name)?;
    do_stop(&meta.name)
}

fn supervisor_stop_and_disable(instance: &str) -> anyhow::Result<bool> {
    if cfg!(target_os="macos") {
        macos::stop_and_disable(&instance)
    } else if cfg!(target_os="linux") {
        linux::stop_and_disable(&instance)
    } else if cfg!(windows) {
        windows::stop_and_disable(&instance)
    } else {
        anyhow::bail!("service is not supported on the platform");
    }
}

pub fn stop_and_disable(instance: &str) -> anyhow::Result<bool> {
    let lock_path = lock_file(instance)?;
    let supervisor = detect_supervisor(instance);
    if lock_path.exists() {
        let lock = open_lock(instance)?;
        if lock.try_read().is_err() {  // properly running
            if !supervisor || !is_run_by_supervisor(lock) {
                if let Some(pid) = read_pid(instance)? {
                    log::info!("Killing EdgeDB with pid {}", pid);
                    process::term(pid)?;
                    // wait for unlock
                    let _ = open_lock(instance)?.read()?;
                }
            }
        }
    }
    if supervisor {
        supervisor_stop_and_disable(instance)
    } else {
        let dir = runstate_dir(instance)?;
        Ok(dir.exists())
    }
}

fn supervisor_restart(inst: &InstanceInfo) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::restart_service(inst)
    } else if cfg!(target_os="macos") {
        macos::restart_service(inst)
    } else if cfg!(target_os="linux") {
        linux::restart_service(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn do_restart(inst: &InstanceInfo) -> anyhow::Result<()> {
    let lock = open_lock(&inst.name)?;
    let supervisor = detect_supervisor(&inst.name);
    if lock.try_read().is_err() {  // properly running
        if supervisor && is_run_by_supervisor(lock) {
            supervisor_restart(inst)
        } else {
            if let Some(pid) = read_pid(&inst.name)? {
                log::info!("Killing EdgeDB with pid {}", pid);
                process::term(pid)?;
                // wait for unlock
                let _ = open_lock(&inst.name)?.read()?;
            } else {
                return Err(bug::error("cannot find pid"));
            }
            if supervisor {
                supervisor_start(inst)
            } else {
                daemon_start(&inst.name)
            }
        }
    } else {  // probably not running
        if supervisor {
            supervisor_restart(inst)
        } else {
            if let Some(pid) = read_pid(&inst.name)? {
                log::info!("Killing EdgeDB with pid {}", pid);
                process::term(pid)?;
                // wait for unlock
                let _ = lock.read()?;
            } // nothing to do
            // todo(tailhook) optimize supervisor detection
            if supervisor {
                supervisor_start(inst)
            } else {
                daemon_start(&inst.name)
            }
        }
    }
}

pub fn restart(options: &Restart) -> anyhow::Result<()> {
    let meta = InstanceInfo::read(&options.name)?;
    do_restart(&meta)
}

pub fn logs(options: &Logs) -> anyhow::Result<()> {
    if cfg!(windows) {
        windows::logs(options)
    } else if cfg!(target_os="macos") {
        macos::logs(options)
    } else if cfg!(target_os="linux") {
        linux::logs(options)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn self_signed_arg(cmd: &mut process::Native, ver: &ver::Build) {
    if ver.specific() > "1.0-rc.2".parse().unwrap() {
        cmd.arg("--tls-cert-mode=generate_self_signed");
    } else {
        cmd.arg("--generate-self-signed-cert");
    }
}
