use crate::docker;
use test_case::test_case;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM centos:{codename}
        RUN yum -y install sudo
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --group users \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test_case(7, false)]
#[test_case(8, false)]
#[test_case(7, true)]
#[test_case(8, true)]
fn sudo_install(release: u32, nightly: bool)
    -> Result<(), anyhow::Error>
{
    docker::sudo_test(
        &dockerfile(&format!("{}", release)),
        &format!("edgedb_test:centos{}_sudo", release),
        nightly)
}

// Only works on nightly, because other overwrite edgedb command
#[test_case(8, true)]
fn refuse_to_reinstall(release: u32, nightly: bool)
    -> Result<(), anyhow::Error>
{
    docker::install_twice_test(
        &dockerfile(&format!("{}", release)),
        &format!("edgedb_test:centos{}_sudo", release),
        nightly)
}
