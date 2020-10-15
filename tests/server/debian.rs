use crate::docker;
use test_case::test_case;


pub fn dockerfile(codename: &str) -> String {
    format!(r###"
        FROM debian:{codename}
        RUN apt-get update
        RUN apt-get install -y ca-certificates sudo gnupg2 apt-transport-https
        RUN adduser --uid 1000 --home /home/user1 \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user1
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test_case("buster", false)]
#[test_case("stretch", false)]
#[test_case("buster", true)]
#[test_case("stretch", true)]
fn sudo_install(codename: &str, nightly: bool) -> Result<(), anyhow::Error> {
    docker::sudo_test(
        &dockerfile(codename),
        &format!("edgedb_test:{}_sudo", codename),
        nightly)
}
