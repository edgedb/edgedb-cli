use test_case::test_case;

use crate::measure::Time;
use crate::docker::{Context, build_image};
use crate::docker::{run_docker, run_systemd};
use crate::common::{dock_ubuntu, dock_centos, dock_debian};

#[test_case("edbtest_centos7", &dock_centos(7))]
fn package_no_systemd(tagname: &str, dockerfile: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(tagname, r###"
        edgedb instance create test1 --version=1-rc.5 --start-conf=manual
        edgedb instance start --foreground test1 &
        edgedb --wait-until-available=60s -Itest1 query '
            CREATE TYPE Type1 {
                CREATE PROPERTY prop1 -> str;
            }
        ' 'INSERT Type1 { prop1 := "value1" }'
        kill %1 && wait

        RUST_LOG=debug edgedb instance upgrade test1 --to-version=1

        edgedb instance start --foreground test1 &
        val=$(edgedb -Itest1 --wait-until-available=60s --tab-separated \
              query 'SELECT Type1 { prop1 }')
        test "$val" = "value1"
    "###).success();
    Ok(())
}

#[test_case("edbtest_bionic", &dock_ubuntu("bionic"))]
#[test_case("edbtest_xenial", &dock_ubuntu("xenial"))]
#[test_case("edbtest_focal", &dock_ubuntu("focal"))]
#[test_case("edbtest_centos8", &dock_centos(8))]
#[test_case("edbtest_buster", &dock_debian("buster"))]
#[test_case("edbtest_stretch", &dock_debian("stretch"))]
fn package(tagname: &str, dockerfile: &str) -> anyhow::Result<()> {
    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, tagname)?;
    run_systemd(tagname, r###"
        edgedb server install --version=1-rc.5
        edgedb instance create test1 --version=1-rc.5

        ver1=$(edgedb -Itest1 --wait-until-available=60s --tab-separated query '
            SELECT sys::get_version_as_str()
        ')
        [[ $ver1 =~ ^1\.0-rc\.5\+ ]]

        edgedb --wait-until-available=60s -Itest1 query '
            CREATE TYPE Type1 {
                CREATE PROPERTY prop1 -> str;
            }
        ' 'INSERT Type1 { prop1 := "value1" }'
        if ! edgedb instance upgrade test1 --to-version=1; then
            res=$?
            journalctl -xe
            exit $res
        fi
        ver2=$(edgedb -Itest1 --wait-until-available=60s --tab-separated query '
            SELECT sys::get_version_as_str()
        ')
        [[ $ver2 =~ ^1\.[0-9]+\+ ]]

        val=$(edgedb -Itest1 --wait-until-available=60s --tab-separated \
              query 'SELECT Type1 { prop1 }')
        test "$val" = "value1"

        if ! edgedb server revert test1 --no-confirm; then
            res=$?
            journalctl -xe
            exit $res
        fi
        ver2=$(edgedb -Itest1 --wait-until-available=60s --tab-separated query '
            SELECT sys::get_version_as_str()
        ')
        [[ $ver1 =~ ^1\.0-rc\.5\+ ]]

        val=$(edgedb -Itest1 --wait-until-available=60s --tab-separated \
              query 'SELECT Type1 { prop1 }')
        test "$val" = "value1"
    "###).success();
    Ok(())
}
