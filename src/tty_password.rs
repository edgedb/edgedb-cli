use anyhow::Context;

use crate::interrupt::MemorizeTerm;

pub fn read(prompt: impl AsRef<str>) -> anyhow::Result<String> {
    let _term = MemorizeTerm::new()?;
    let passwd = rpassword::prompt_password(prompt.as_ref())
        .context("error reading password")?;
    Ok(passwd)
}
