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

#[test]
fn bionic_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("bionic"),
        "edgedb_server_test:bionic_sudo",
        false)
}

#[test]
fn xenial_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("xenial"),
        "edgedb_server_test:xenial_sudo",
        false)
}

#[test]
fn bionic_sudo_nightly() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("bionic"),
        "edgedb_server_test:bionic_sudo",
        true)
}

#[test]
fn xenial_sudo_nightly() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("xenial"),
        "edgedb_server_test:xenial_sudo",
        true)
}

#[test]
fn focal_sudo_nightly() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("xenial"),
        "edgedb_server_test:xenial_sudo",
        true)
}
