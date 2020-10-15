use test_case::test_case;
use predicates::str::contains;

use crate::docker::{Context, build_image};
use crate::docker::{run, run_with_socket, run_systemd};
use crate::common::{dock_ubuntu, dock_centos, dock_debian};


#[test_case("edbtest_centos7", &dock_centos(7))]
fn package_no_systemd(tagname: &str, dockerfile: &str)
    -> anyhow::Result<()>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run(tagname, r###"
        edgedb server install
        edgedb server init test1
    "###).code(2)
        .stderr(contains("Bootstrapping complete"))
        .stderr(contains("start --foreground"));
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"))]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"))]
#[test_case("edbtest_focal", &dock_ubuntu("focal"))]
#[test_case("edbtest_centos8", &dock_centos(8))]
#[test_case("edbtest_buster", &dock_debian("buster"))]
#[test_case("edbtest_stretch", &dock_debian("stretch"))]
fn package(tagname: &str, dockerfile: &str) -> anyhow::Result<()> {
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(tagname, r###"
        edgedb server install
        edgedb server init test1
        val=$(edgedb -Itest1 --wait-until-available=30s query "SELECT 1+1")
        test "$val" = "2"
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
        docker ps -q -f 'name=edgedb_test' | xargs -r docker container kill
        docker system prune --all --force
        docker volume list -q -f 'name=edgedb_test' | xargs -r docker volume rm

        edgedb server install --nightly --method=docker
    "###).success();
    Ok(())
}
