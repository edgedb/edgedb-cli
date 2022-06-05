#![cfg_attr(not(feature="portable_tests"), allow(dead_code, unused_imports))]

use assert_cmd::Command;
use predicates::prelude::*;

mod util;
use util::*;


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

    Command::new("edgedb")
        .arg("server").arg("list-versions")
        .assert()
        .context("list-versions-before", "list with no installed")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("create").arg("inst1")
        .assert()
        .context("create-1", "created `inst1`")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .current_dir("tests/proj/project1")
        .assert()
        .context("project-info-no", "not initialied")
        .code(1)
        .stderr(predicates::str::contains("is not initialized"));

    Command::new("edgedb")
        .arg("project").arg("init").arg("--link")
        .arg("--server-instance=inst1")
        .arg("--non-interactive")
        .current_dir("tests/proj/project1")
        .assert()
        .context("project-link", "linked `inst1` to project project1")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .current_dir("tests/proj/project1")
        .assert()
        .context("project-info", "instance-name == inst1")
        .success()
        .stdout(predicates::ord::eq("inst1\n"));

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project1")
        .assert()
        .context("query-1", "query of project")
        .success();

    Command::new("edgedb").arg("project").arg("init")
        .arg("--non-interactive")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-init", "init project2")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-info", "instance-name == project2")
        .success()
        .stdout(predicates::ord::eq("project2\n"));

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-2", "query of project2")
        .success();

    Command::new("edgedb").arg("project").arg("upgrade")
        .arg("--force")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-upgrade", "upgrade project")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-3", "query after upgrade")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("logs")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-logs", "project instance logs")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("restart")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-restart", "project instance restart")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-4", "query after restart")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("stop")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-stop", "project instance stop")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("start")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-start", "project instance start")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-5", "query after manual stop/start")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("reset-password")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-reset-password", "project instance reset password")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("query-6", "query after password reset")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2")
        .arg("--non-interactive")
        .assert()
        .context("destroy-2-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("inst1")
        .arg("--non-interactive")
        .assert()
        .context("destroy-1-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project1")
        .arg("--non-interactive")
        .assert()
        .context("destroy-1-non-exist", "it's project name, not instance name")
        .code(8); // instance not found

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("instance-list-1", "list two instances")
        .success()
        .stdout(predicates::str::contains("inst1"))
        .stdout(predicates::str::contains("project2"));

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2")
        .arg("--force")
        .assert()
        .context("destroy-2", "should destroy")
        .success();

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("instance-list-2", "list once instance")
        .success()
        .stdout(predicates::str::contains("inst1"))
        .stdout(predicates::str::contains("project2").not());

    Command::new("edgedb")
        .arg("project").arg("unlink").arg("-D").arg("--non-interactive")
        .current_dir("tests/proj/project1")
        .assert()
        .context("destroy-1", "should unlink and destroy project")
        .success();

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("instance-list-3", "list no instances")
        .success()
        .stdout(predicates::str::contains("inst1").not())
        .stdout(predicates::str::contains("project2").not());

    Command::new("edgedb").arg("project").arg("init")
        .arg("--non-interactive")
        .arg("--server-start-conf=manual")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-init-manual", "init project2 manual")
        .success();

    Command::new("edgedb").arg("project").arg("upgrade")
        .arg("--to-latest").arg("--force")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-upgrade-manual", "upgrade manual project")
        .success();

    Command::new("edgedb").arg("project").arg("instance").arg("status")
        .arg("--extended")
        .current_dir("tests/proj/project2")
        .assert()
        .context("project-instance-status", "show extended status")
        .code(3);

    Command::new("edgedb").arg("instance").arg("revert").arg("project2")
        .arg("--no-confirm")
        .assert()
        .context("project-revert-manual", "revert manual project")
        .success();
}
