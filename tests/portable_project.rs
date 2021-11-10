#![cfg_attr(not(feature="portable_tests"), allow(dead_code))]

use assert_cmd::Command;
mod util;

use util::*;


#[cfg(feature="portable_tests")]
#[test]
fn project_link() {
    Command::new("edgedb")
        .arg("--version")
        .assert()
        .context("version", "command-line version option")
        .success()
        .stdout(predicates::str::contains(
            concat!("EdgeDB CLI ", env!("CARGO_PKG_VERSION"))));

    // only nightly works so far
    Command::new("edgedb")
        .arg("instance").arg("create").arg("inst1").arg("--nightly")
        .assert()
        .context("create-1", "created `inst1`")
        .success();

    Command::new("edgedb")
        .arg("project").arg("init").arg("--link")
        .arg("--server-instance=inst1")
        .arg("--non-interactive")
        .current_dir("tests/proj/project1")
        .assert()
        .context("project-link", "linked `inst1` to project project1")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project1")
        .assert()
        .context("query-1", "query of project")
        .success();

    // only nightly works so far
    Command::new("edgedb").arg("project").arg("init")
        .arg("--server-version=nightly").arg("--non-interactive")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-init", "init project2")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-2", "query of project2")
        .success();
}
