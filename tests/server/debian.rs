use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM debian:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo gnupg2 apt-transport-https
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test]
fn buster_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("buster"),
        "edgedb_server_test:buster_sudo", false)
}

#[test]
fn stretch_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("stretch"),
        "edgedb_server_test:stretch_sudo", false)
}

#[test]
fn buster_sudo_nightly() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("buster"),
        "edgedb_server_test:buster_sudo", true)
}

#[test]
fn stretch_sudo_nightly() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("stretch"),
        "edgedb_server_test:stretch_sudo", true)
}
