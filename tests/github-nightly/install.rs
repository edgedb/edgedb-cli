use predicates::str::contains;
use test_case::test_case;

use crate::common::{dock_debian, dock_ubuntu, dock_ubuntu_jspy};
use crate::docker::{build_image, Context};
use crate::docker::{run, run_docker, run_systemd};
use crate::measure::Time;

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--nightly")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--nightly")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--nightly")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--nightly")]
fn package(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(
        tagname,
        &format!(
            r###"
            edgedb server install {version}
            edgedb instance create test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"
            edgedb instance logs test1
            timeout 180 edgedb instance destroy test1
            edgedb server uninstall --all --verbose
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}

#[test_case("edbtest_focal", &dock_ubuntu_jspy("focal"), "")]
#[test_case("edbtest_focal", &dock_ubuntu_jspy("focal"), "--nightly")]
fn package_jspy(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_edbconnect()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(
        tagname,
        &format!(
            r###"
            edgedb server install {version}
            edgedb instance create test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"
            python3 ./edbconnect.py test1
            node ./edbconnect.js test1
            edgedb instance logs test1
            timeout 180 edgedb instance destroy test1
            edgedb server uninstall --all --verbose
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--nightly")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--nightly")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--nightly")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--nightly")]
fn docker(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_docker(
        tagname,
        &format!(
            r###"
            edgedb server install {version}
            RUST_LOG=info edgedb instance create test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"
            edgedb instance logs test1
            timeout 180 edgedb instance destroy test1
            edgedb server uninstall --all --verbose
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}

#[test_case("edbtest_focal", &dock_ubuntu_jspy("focal"), "")]
#[test_case("edbtest_focal", &dock_ubuntu_jspy("focal"), "--nightly")]
fn docker_jspy(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_edbconnect()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_docker(
        tagname,
        &format!(
            r###"
            edgedb server install {version}
            edgedb instance create test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"
            python3 ./edbconnect.py test1
            node ./edbconnect.js test1
            edgedb instance logs test1
            timeout 180 edgedb instance destroy test1
            edgedb server uninstall --all --verbose
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}
