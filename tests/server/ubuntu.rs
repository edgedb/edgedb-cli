use std::io::Write;

use assert_cmd::Command;

use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates
        ADD ./edgedb /usr/bin/edgedb
        RUN edgedb server install
        RUN edgedb-server --version
    "###, codename=codename)
}

#[test]
fn straightforward() -> Result<(), anyhow::Error> {
    let context = docker::make_context(&dockerfile("bionic"))?;
    // Put this into a file to make error reporting simpler
    // (rust prints whole stdin vector otherwise)
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(&context)?;
    Command::new("docker")
        .args(&["build", "-"])
        .pipe_stdin(tmp.path())?
        .assert()
        .success();
    Ok(())
}
