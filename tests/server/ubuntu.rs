use std::io::Write;

use assert_cmd::Command;

use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

pub fn sudoers() -> &'static str {
    r###"
        root        ALL=(ALL:ALL) SETENV: ALL
        daemon	ALL=(ALL:ALL)	NOPASSWD: ALL
    "###
}

#[test]
fn bionic_sudo() -> Result<(), anyhow::Error> {
    let context = docker::make_context(&dockerfile("bionic"), sudoers())?;
    Command::new("docker")
        .args(&["build", "-", "-t", "bionic_sudo"])
        .write_stdin(context)
        .assert()
        .success();
    Command::new("docker")
        .args(&["run", "--rm", "-u", "1", "bionic_sudo:latest"])
        .args(&["sh", "-exc", r###"
            edgedb server install
            echo --- DONE ---
            edgedb-server --version
        "###])
        .assert()
        .success();
    Ok(())
}
