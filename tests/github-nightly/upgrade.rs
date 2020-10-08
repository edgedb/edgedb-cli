use test_case::test_case;

use crate::docker::{Context, build_image, run, run_with_socket};


pub fn dock_ubuntu(codename: &str) -> String {
    format!(r###"
        FROM ubuntu:{codename}
        RUN apt-get update && apt-get install -y ca-certificates sudo gnupg2 apt-transport-https curl software-properties-common
        RUN curl -fsSL https://download.docker.com/linux/ubuntu/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/ubuntu \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

pub fn dock_centos(codename: u32) -> String {
    format!(r###"
        FROM centos:{codename}
        RUN yum -y install sudo yum-utils
        RUN yum-config-manager \
            --add-repo \
            https://download.docker.com/linux/centos/docker-ce.repo
        RUN yum -y install docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --group users \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

pub fn dock_debian(codename: &str) -> String {
    format!(r###"
        FROM debian:{codename}
        RUN apt-get update && apt-get install -y ca-certificates sudo gnupg2 apt-transport-https curl software-properties-common
        RUN curl -fsSL https://download.docker.com/linux/debian/gpg | apt-key add -
        RUN add-apt-repository \
           "deb [arch=amd64] https://download.docker.com/linux/debian \
           $(lsb_release -cs) \
           stable"
        RUN apt-get update && apt-get install -y docker-ce-cli
        RUN adduser --uid 1000 --home /home/user \
            --shell /bin/bash --ingroup users --gecos "EdgeDB Test User" \
            user
        ADD ./edgedb /usr/bin/edgedb
        ADD ./sudoers /etc/sudoers
    "###, codename=codename)
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"))]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"))]
#[test_case("edbtest_focal", &dock_ubuntu("focal"))]
#[test_case("edbtest_centos7", &dock_centos(7))]
#[test_case("edbtest_centos8", &dock_centos(8))]
#[test_case("edbtest_buster", &dock_debian("buster"))]
#[test_case("edbtest_stretch", &dock_debian("stretch"))]
fn package(tagname: &str, dockerfile: &str) -> anyhow::Result<()> {
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run(tagname, r###"
        edgedb server install
    "###).success();
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"))]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"))]
#[test_case("edbtest_focal", &dock_ubuntu("focal"))]
#[test_case("edbtest_centos7", &dock_centos(7))]
#[test_case("edbtest_centos8", &dock_centos(8))]
#[test_case("edbtest_buster", &dock_debian("buster"))]
#[test_case("edbtest_stretch", &dock_debian("stretch"))]
fn docker(tagname: &str, dockerfile: &str) -> anyhow::Result<()> {
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_with_socket(tagname, r###"
        docker container prune --force
        docker image prune --all --force
        edgedb server install --nightly --method=docker
    "###).success();
    Ok(())
}
