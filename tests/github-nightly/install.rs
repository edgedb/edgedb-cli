use test_case::{test_case, test_matrix};

use crate::common::{dock_ubuntu_jspy, Distro};
use crate::docker::{build_image, Context};
use crate::docker::{run_docker, run_systemd};
use crate::measure::Time;

#[test_matrix(
    [
        Distro::Ubuntu("focal"),
        Distro::Ubuntu("bionic"),
        Distro::Debian("bookworm"),
        Distro::Debian("bullseye"),
    ],
    ["", "--nightly"]
)]
fn package(distro: Distro, version: &str) -> anyhow::Result<()> {
    let tag_name = distro.tag_name();

    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", distro.dockerfile())?
        .add_sudoers()?
        .add_bin()?;
    build_image(context, &tag_name)?;
    run_systemd(
        &tag_name,
        &format!(
            r###"
            edgedb server install {version}
            edgedb instance create test1 {version}
            val=$(edgedb -Itest1 --wait-until-available=60s \
                query "SELECT 1+1")
            test "$val" = "2"
            edgedb instance logs -I test1
            timeout 180 edgedb instance destroy -I test1 --non-interactive
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
            edgedb instance logs -I test1
            timeout 180 edgedb instance destroy -I test1 --non-interactive
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
            edgedb instance logs -I test1
            timeout 180 edgedb instance destroy -I test1 --non-interactive
            edgedb server uninstall --all --verbose
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}
