use std::io;
use std::path::PathBuf;
use std::process::{Command, Child};


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
