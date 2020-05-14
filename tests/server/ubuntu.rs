use assert_cmd::Command;

use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo gnupg2
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
            RUST_LOG=info edgedb server install
            echo --- DONE ---
            edgedb-server --help
            apt-cache policy edgedb-1-alpha2
        "###])
        // add edgedb-server --version check
        .assert()
        .success()
        .stdout(predicates::str::contains("--- DONE ---"))
        .stdout(predicates::function::function(|data: &str| {
            let tail = &data[data.find("--- DONE ---").unwrap()..];
            assert!(tail.contains("Usage: edgedb-server [OPTIONS]"));
            assert!(tail.contains("Installed: 1.0a2"));
            true
        }));
    Ok(())
}
