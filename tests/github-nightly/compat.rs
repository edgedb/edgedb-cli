use test_case::test_case;

use crate::docker::{Context, build_image};
use crate::docker::{run_systemd};
use crate::common::{dock_ubuntu, dock_centos, dock_debian};

// stable
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "")]
#[test_case("edbtest_centos8", &dock_centos(8), "")]
#[test_case("edbtest_buster", &dock_debian("buster"), "")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "")]
// alpha7
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--version=1-alpha7")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--version=1-alpha7")]
#[test_case("edbtest_centos8", &dock_centos(8), "--version=1-alpha7")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--version=1-alpha7")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--version=1-alpha7")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "--version=1-alpha7")]
// nightly
#[test_case("edbtest_bionic", &dock_ubuntu("bionic"), "--nightly")]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"), "--nightly")]
#[test_case("edbtest_centos8", &dock_centos(8), "--nightly")]
#[test_case("edbtest_buster", &dock_debian("buster"), "--nightly")]
#[test_case("edbtest_stretch", &dock_debian("stretch"), "--nightly")]
#[test_case("edbtest_focal", &dock_ubuntu("focal"), "--nightly")]
fn cli(tagname: &str, dockerfile: &str, version: &str)
    -> anyhow::Result<()>
{
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(tagname, &format!(r###"
            edgedb server install {version}
            edgedb server init test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"

            # changed in 1-alpha.7 due to dropping implicit __tid__
            edgedb -Itest1 list-scalar-types --system
        "###,
        version=version,
    )).success();
    Ok(())
}
