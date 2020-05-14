use assert_cmd::Command;

use std::fs;
use std::env;

use tar::{Builder, Header};

fn sudoers() -> &'static str {
    r###"
        root        ALL=(ALL:ALL) SETENV: ALL
        daemon	ALL=(ALL:ALL)	NOPASSWD: ALL
    "###
}

fn make_context(dockerfile: &str, sudoers: &str)
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

pub fn sudo_test(dockerfile: &str, tagname: &str,
    major_ver: &str, display_ver: &str)
    -> Result<(), anyhow::Error>
{
    let context = make_context(&dockerfile, sudoers())?;
    Command::new("docker")
        .arg("build").arg("-")
        .arg("-t").arg(tagname)
        .write_stdin(context)
        .assert()
        .success();
    Command::new("docker")
        .args(&["run", "--rm", "-u", "1"])
        .arg(tagname)
        .args(&["sh", "-exc", &format!(r###"
            RUST_LOG=info edgedb server install
            echo --- DONE ---
            edgedb-server --help
            apt-cache policy edgedb-{}
        "###, major_ver)])
        // add edgedb-server --version check
        .assert()
        .success()
        .stdout(predicates::str::contains("--- DONE ---"))
        .stdout(predicates::function::function(|data: &str| {
            let tail = &data[data.find("--- DONE ---").unwrap()..];
            assert!(tail.contains("Usage: edgedb-server [OPTIONS]"));
            assert!(tail.contains(&format!("Installed: {}", display_ver)));
            true
        }));
    Ok(())
}

