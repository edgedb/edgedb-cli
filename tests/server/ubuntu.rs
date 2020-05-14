use assert_cmd::Command;

use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo gnupg2 apt-transport-https
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

pub fn test_sudo(codename: &'static str,
    major_ver: &'static str, display_ver: &'static str)
    -> Result<(), anyhow::Error>
{
    let context = docker::make_context(&dockerfile(codename), sudoers())?;
    Command::new("docker")
        .args(&["build", "-", "-t", &format!("{}_sudo", codename)])
        .write_stdin(context)
        .assert()
        .success();
    Command::new("docker")
        .args(&["run", "--rm", "-u", "1"])
        .arg(format!("{}_sudo:latest", codename))
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

#[test]
fn bionic_sudo_alpha2() -> Result<(), anyhow::Error> {
    test_sudo("bionic", "1-alpha2", "1.0a2")
}

#[test]
fn xenial_sudo_alpha2() -> Result<(), anyhow::Error> {
    test_sudo("xenial", "1-alpha2", "1.0a2")
}
