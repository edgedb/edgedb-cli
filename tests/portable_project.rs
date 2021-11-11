#![cfg_attr(not(feature="portable_tests"), allow(dead_code))]


mod util;




#[cfg(feature="portable_tests")]
#[test]
fn project_link_and_init() {
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

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2")
        .assert()
        .context("destroy-2-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("inst1")
        .assert()
        .context("destroy-1-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project1")
        .assert()
        .context("destroy-1-non-exist", "it's project name, not instance name")
        .code(1);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2").arg("--force")
        .assert()
        .context("destroy-2", "should destroy")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("inst1").arg("--force")
        .assert()
        .context("destroy-1", "should destroy")
        .success();
}
