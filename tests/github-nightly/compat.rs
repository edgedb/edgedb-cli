use test_case::test_matrix;

use crate::common::Distro;
use crate::docker::run_systemd;
use crate::docker::{build_image, Context};
use crate::measure::Time;

#[test_matrix(
    [
        Distro::Ubuntu("focal"),
        Distro::Ubuntu("bionic"),
        Distro::Ubuntu("xenial"),
        Distro::Debian("bookworm"),
        Distro::Debian("bullseye"),
        Distro::Debian("buster"),
    ],
    [
        "", // latest
        "--version=4.8", // previous
        "--nightly", // nightly
    ]
)]
fn cli(distro: Distro, version: &str) -> anyhow::Result<()> {
    let dockerfile = distro.dockerfile();
    let tag_name = distro.tag_name();

    let _tm = Time::measure();
    let context = Context::new()
        .add_file("Dockerfile", dockerfile)?
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

            edgedb -Itest1 list scalars --system
        "###,
            version = version,
        ),
    )
    .success();
    Ok(())
}
