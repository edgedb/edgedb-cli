use test_case::test_case;
use predicates::str::contains;

use crate::docker::{Context, build_image};
use crate::docker::{run, run_docker, run_systemd};
use crate::common::{dock_ubuntu, dock_centos, dock_debian};


#[test_case("edbtest_centos7", &dock_centos(7))]
fn package_no_systemd(tagname: &str, dockerfile: &str)
    -> anyhow::Result<()>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_edbconnect()?
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

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "")]
#[test_case("edbtest_centos8", &dock_centos(8), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--nightly")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--nightly")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "--nightly")]
#[test_case("edbtest_centos8", &dock_centos(8), "--nightly")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--nightly")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--nightly")]
fn package(tagname: &str, dockerfile: &str, version: &str)
    -> anyhow::Result<()>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_edbconnect()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(tagname, &format!(r###"
            edgedb server install {version}
            edgedb server init test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=30s \
                query "SELECT 1+1")
            test "$val" = "2"
            python3 /usr/bin/edbconnect.py test1
            edgedb server logs test1
            edgedb server destroy test1
            edgedb server uninstall --all --verbose
        "###,
        version=version,
    )).success();
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "")]
#[test_case("edbtest_centos7", &dock_centos(7), "")]
#[test_case("edbtest_centos8", &dock_centos(8), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--nightly")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--nightly")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "--nightly")]
#[test_case("edbtest_centos7", &dock_centos(7), "--nightly")]
#[test_case("edbtest_centos8", &dock_centos(8), "--nightly")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--nightly")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--nightly")]
fn docker(tagname: &str, dockerfile: &str, version: &str)
    -> anyhow::Result<()>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_edbconnect()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_docker(tagname, &format!(r###"
            edgedb server install --method=docker {version}
            edgedb server init test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=30s \
                query "SELECT 1+1")
            test "$val" = "2"
            python3 /usr/bin/edbconnect.py test1
            edgedb server logs test1
            edgedb server destroy test1
            edgedb server uninstall --all --verbose
        "###,
        version=version,
    )).success();
    Ok(())
}
