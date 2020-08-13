use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Child};
use std::ffi::OsString;


#[cfg(windows)]
pub type Uid = u32;

#[cfg(not(windows))]
pub type Uid = libc::uid_t;

pub struct ProcessGuard {
    child: Child,
}

#[cfg(windows)]
pub fn get_current_uid() -> Uid {
    unreachable!();
}

#[cfg(not(windows))]
pub fn get_current_uid() -> Uid {
    unsafe { libc::geteuid() }
}

pub fn home_dir() -> anyhow::Result<PathBuf> {
    dirs::home_dir()
    .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))
}

pub fn config_dir() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".edgedb").join("config"))
}

pub fn tmp_file_name(path: &Path) -> OsString {
    if let Some(file_name) = path.file_name() {
        let mut buf = OsString::with_capacity(6 + file_name.len());
        buf.push(".~");
        buf.push(file_name);
        buf.push(".tmp");
        buf
    } else {
        OsString::from(".~.tmp")  // should never be relied on in practice
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
