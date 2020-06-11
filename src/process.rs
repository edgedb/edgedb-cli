use std::process::{Command, exit};

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
