use std::path::{Path, PathBuf};
use std::ffi::OsString;


#[cfg(windows)]
pub type Uid = u32;

#[cfg(not(windows))]
pub type Uid = libc::uid_t;

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
