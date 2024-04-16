use test_case::test_case;

use crate::common::{dock_centos, dock_debian, dock_ubuntu};
use crate::docker::{build_image, Context};
use crate::docker::{run_docker, run_systemd};
use crate::measure::Time;

const NIGHTLY: &str = "--server-version=nightly";

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_centos8", &dock_centos(8), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), NIGHTLY)]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), NIGHTLY)]
#[test_case("edbtest_centos8", &dock_centos(8), NIGHTLY)]
#[test_case("edbtest_buster", &dock_debian("buster"), NIGHTLY)]
#[test_case("edbtest_stretch", &dock_debian("stretch"), NIGHTLY)]
fn simple_package(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
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
            mkdir -p /tmp/test1
            cd /tmp/test1
            edgedb project init --non-interactive \
                --server-install-method=package \
                {version}
            val=$(edgedb --wait-until-available=60s query "SELECT 7+8")
            test "$val" = "15"
            timeout 120 edgedb project unlink \
                --destroy-server-instance --non-interactive
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_centos8", &dock_centos(8), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), NIGHTLY)]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), NIGHTLY)]
#[test_case("edbtest_centos8", &dock_centos(8), NIGHTLY)]
#[test_case("edbtest_buster", &dock_debian("buster"), NIGHTLY)]
#[test_case("edbtest_stretch", &dock_debian("stretch"), NIGHTLY)]
fn simple_docker(tagname: &str, dockerfile: &str, version: &str) -> anyhow::Result<()> {
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
            mkdir -p /tmp/test1
            cd /tmp/test1
            edgedb project init --non-interactive \
                --server-install-method=docker \
                {version}
            val=$(edgedb --wait-until-available=60s query "SELECT 7+8")
            test "$val" = "15"
            edgedb project unlink --destroy-server-instance --non-interactive
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}
