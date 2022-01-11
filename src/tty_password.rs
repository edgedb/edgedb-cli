use anyhow::Context;

use crate::interrupt::MemorizeTerm;

pub fn read(prompt: impl AsRef<str>) -> anyhow::Result<String> {
    let _term = MemorizeTerm::new()?;
    let passwd = rpassword::read_password_from_tty(Some(prompt.as_ref()))
        .context("error reading password")?;
    Ok(passwd)
}
