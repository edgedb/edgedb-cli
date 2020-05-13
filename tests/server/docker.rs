use std::fs;
use std::env;

use tar::{Builder, Header};

pub fn make_context(dockerfile: &str, sudoers: &str)
    -> Result<Vec<u8>, anyhow::Error>
{
    let buf = Vec::with_capacity(1048576);
    let mut arch = Builder::new(buf);

    let mut header = Header::new_gnu();
    header.set_size(dockerfile.len() as u64);
    header.set_path("Dockerfile")?;
    header.set_cksum();
    arch.append(&header, dockerfile.as_bytes())?;

    let mut header = Header::new_gnu();
    header.set_size(sudoers.len() as u64);
    header.set_path("sudoers")?;
    header.set_cksum();
    arch.append(&header, sudoers.as_bytes())?;

    let bin = fs::read(env!("CARGO_BIN_EXE_edgedb"))?;
    let mut header = Header::new_gnu();
    header.set_size(bin.len() as u64);
    header.set_path("edgedb")?;
    header.set_mode(0o755);
    header.set_cksum();
    arch.append(&header, &bin[..])?;

    arch.finish()?;
    Ok(arch.into_inner()?)
}
