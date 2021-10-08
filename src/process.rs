use std::process::{Command, exit};

use anyhow::Context;
use serde::de::DeserializeOwned;


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

pub fn run_or_stderr(cmd: &mut Command) -> anyhow::Result<Result<(), String>> {
    match cmd.output() {
        Ok(child) if child.status.success() => Ok(Ok(())),
        Ok(out) => {
            Ok(Err(String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("cannot decode error output of {:?}", cmd)
                })?))
        }
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd)),
    }
}

pub fn exit_from(cmd: &mut Command) -> anyhow::Result<()> {
    match cmd.status() {
        Ok(s) if s.code().is_some() => exit(s.code().unwrap()),
        Ok(s) => anyhow::bail!("process {:?} failed: {}", cmd, s),
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd)),
    }
}

pub fn get_text(cmd: &mut Command) -> anyhow::Result<String> {
    let data = match cmd.output() {
        Ok(out) if out.status.success() => out.stdout,
        Ok(out) => anyhow::bail!("process {:?} failed: {}", cmd, out.status),
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd))?,
    };
    String::from_utf8(data)
        .with_context(|| format!("cannot decode output of {:?}", cmd))
}

pub fn get_json_or_stderr<T: DeserializeOwned>(cmd: &mut Command)
    -> anyhow::Result<Result<T, String>>
{
    match cmd.output() {
        Ok(out) if out.status.success() => {
            Ok(Ok(serde_json::from_slice(&out.stdout[..])
                .with_context(|| format!("cannot decode output of {:?}", cmd))?))
        }
        Ok(out) => {
            Ok(Err(String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("cannot decode error output of {:?}", cmd)
                })?))
        }
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd))?,
    }
}
