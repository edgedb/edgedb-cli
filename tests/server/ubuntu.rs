use crate::docker;
use test_case::test_case;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo gnupg2 apt-transport-https
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test_case("bionic", false)]
#[test_case("xenial", false)]
#[test_case("bionic", true)]
#[test_case("xenial", true)]
#[test_case("focal", true; "inconclusive -- no stable version for focal")]
fn sudo_install(codename: &str, nightly: bool)
    -> Result<(), anyhow::Error>
{
    docker::sudo_test(
        &dockerfile(codename),
        &format!("edgedb_test:{}_sudo", codename),
        nightly)
}

// Only works on nightly, because other overwrite edgedb command
#[test_case("xenial", true)]
fn refuse_to_reinstall(codename: &str, nightly: bool)
    -> Result<(), anyhow::Error>
{
    docker::install_twice_test(
        &dockerfile(codename),
        &format!("edgedb_test:{}_sudo", codename),
        nightly)
}
