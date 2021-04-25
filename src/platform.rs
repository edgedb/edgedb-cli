use std::ffi::OsString;
use std::path::{Path, PathBuf};

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
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))
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
        OsString::from(".~.tmp") // should never be relied on in practice
    }
}

pub fn tmp_file_path(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or(&Path::new("."))
        .join(tmp_file_name(path))
}

#[cfg(unix)]
pub fn path_bytes(path: &Path) -> anyhow::Result<&[u8]> {
    use std::os::unix::ffi::OsStrExt;
    return Ok(path.as_os_str().as_bytes());
}

#[cfg(windows)]
pub fn path_bytes<'x>(path: &'x Path) -> anyhow::Result<&'x [u8]> {
    let s = path
        .to_str()
        // should never happen because paths on windows are valid UTF-16
        .ok_or_else(|| anyhow::anyhow!("bad chars in path"))?;
    return Ok(s.as_bytes());
}

#[cfg(unix)]
pub fn bytes_to_path(path: &[u8]) -> anyhow::Result<&Path> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    return Ok(Path::new(OsStr::from_bytes(path)));
}

#[cfg(windows)]
pub fn bytes_to_path<'x>(path: &'x [u8]) -> anyhow::Result<&'x Path> {
    use anyhow::Context;

    let s = std::str::from_utf8(path).context("bad chars in path")?;
    return Ok(Path::new(s));
}

#[cfg(unix)]
pub fn symlink_dir(original: impl AsRef<Path>, path: impl AsRef<Path>) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(original, path)?;
    Ok(())
}

#[cfg(windows)]
pub fn symlink_dir(original: impl AsRef<Path>, path: impl AsRef<Path>) -> anyhow::Result<()> {
    std::os::windows::fs::symlink_dir(original, path)?;
    Ok(())
}
