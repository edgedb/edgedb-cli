use crate::docker;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM centos:{codename}
        RUN yum -y install sudo
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test]
fn centos7_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("7"),
        "edgedb_server_test:centos7_sudo")
}

#[test]
fn centos8_sudo_current() -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile("8"),
        "edgedb_server_test:centos8_sudo")
}
