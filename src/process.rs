use std::process::{Command, exit};

use serde::de::DeserializeOwned;

use anyhow::Context;


pub fn run(cmd: &mut Command) -> anyhow::Result<()> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => anyhow::bail!("process {:?} failed: {}", cmd, s),
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
        .with_context(|| format!("can decode output of {:?}", cmd))
}

pub fn get_json_or_failure<T: DeserializeOwned>(cmd: &mut Command)
    -> anyhow::Result<Result<T, String>>
{
    match cmd.output() {
        Ok(out) if out.status.success() => {
            Ok(Ok(serde_json::from_slice(&out.stdout[..])
                .with_context(|| format!("can decode output of {:?}", cmd))?))
        }
        Ok(out) => {
            Ok(Err(String::from_utf8(out.stderr)
                .with_context(|| {
                    format!("can decode error output of {:?}", cmd)
                })?))
        }
        Err(e) => Err(e).with_context(|| format!("error running {:?}", cmd))?,
    }
}
