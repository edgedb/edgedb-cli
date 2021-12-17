use std::path::{Path, PathBuf};

use anyhow::Context;
use fn_error_context::context;
use fs_err as fs;

use crate::credentials;
use crate::portable::local::{InstanceInfo, runstate_dir, lock_file};
use crate::portable::options::{Start, Stop, Restart, Logs};
use crate::portable::ver;
use crate::portable::{windows, linux, macos};
use crate::process;


pub fn do_start(inst: &InstanceInfo) -> anyhow::Result<()> {
    let cred_path = credentials::path(&inst.name)?;
    if !cred_path.exists() {
        log::warn!("No corresponding credentials file {:?} exists. \
                    Use `edgedb instance reset-password {}` to create one.",
                    cred_path, inst.name);
    }
    if cfg!(windows) {
        windows::start_service(inst)
    } else if cfg!(target_os="macos") {
        macos::start_service(inst)
    } else if cfg!(target_os="linux") {
        linux::start_service(inst)
    } else {
        anyhow::bail!("unsupported platform");
    }
}

pub fn get_server_cmd(inst: &InstanceInfo) -> anyhow::Result<process::Native> {
    if cfg!(windows) {
        windows::server_cmd(inst)
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
    fs::create_dir_all(&runstate_dir)?;
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

#[cfg(unix)]
fn run_server_by_cli(meta: &InstanceInfo) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;
    use std::io;
    use async_std::future::pending;
    use async_std::os::unix::net::UnixDatagram;
    use async_std::task;
    use crate::portable::local::log_file;

    unsafe { libc::setsid() };

    let log_path = log_file(&meta.name)?;
    let log_file = fs::OpenOptions::new()
        .create(true).write(true).append(true)
        .open(&log_path)?;
    let null = fs::OpenOptions::new().write(true).open("/dev/null")?;
    let notify_socket = runstate_dir(&meta.name)?.join(".s.daemon");
    if notify_socket.exists() {
        fs::remove_file(&notify_socket)?;
    }
    if let Some(dir) = notify_socket.parent() {
        fs::create_dir_all(dir)?;
    }
    let sock = task::block_on(UnixDatagram::bind(&notify_socket))
        .context("cannot create notify socket")?;

    get_server_cmd(&meta)?
        .env("NOTIFY_SOCKET", &notify_socket)
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
fn run_server_by_cli() -> anyhow::Result<()> {
    anyhow::bail!("daemonizing is not yet supported for Windows");
}

pub fn start(options: &Start) -> anyhow::Result<()> {
    let meta = InstanceInfo::read(&options.name)?;
    ensure_runstate_dir(&meta.name)?;
    if options.foreground || options.managed_by.is_some() {
        let lock_path = lock_file(&meta.name)?;
        let lock_file = fs::OpenOptions::new()
            .create(true).write(true).read(true)
            .open(&lock_path)
            .with_context(|| format!("cannot open lock file {:?}", lock_path))?;
        let mut lock = fd_lock::RwLock::new(lock_file);
        let mut needs_restart = false;
        let try_write = lock.try_write();
        let lock = if let Ok(mut lock) = try_write {
            write_lock_info(&lock_path, &mut *lock, &options.managed_by)?;
            lock
        } else {
            drop(try_write);
            let locked_by = fs::read_to_string(&lock_path)
                .with_context(|| format!("cannot read lock file {:?}",
                                         lock_path))?;
            if options.managed_by.is_some() {
                log::warn!("Process is already running by {}. \
                            Waiting for that process to be stopped...",
                            locked_by.escape_default());
            } else if options.auto_restart {
                if locked_by != "user" {
                    log::warn!("Process is already running by {}. \
                                Stopping...", locked_by.escape_default());
                    needs_restart = true;
                    do_stop(&options.name)
                        .context("cannot stop service")?;
                } else {
                    log::warn!("Process is already running by {}. \
                                Stopping...", locked_by.escape_default());
                }
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
            let res = get_server_cmd(&meta)?
                .env_default("EDGEDB_SERVER_LOG_LEVEL", "info")
                .no_proxy()
                .run();
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

pub fn do_stop(name: &str) -> anyhow::Result<()> {
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

pub fn stop(options: &Stop) -> anyhow::Result<()> {
    let meta = InstanceInfo::read(&options.name)?;
    do_stop(&meta.name)
}

pub fn do_restart(inst: &InstanceInfo) -> anyhow::Result<()> {
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
