#![cfg_attr(not(feature="portable_tests"), allow(dead_code, unused_imports))]

use assert_cmd::Command;
use predicates::prelude::*;

mod util;
use util::*;


#[cfg(feature="portable_tests")]
#[test]
fn project_link_and_init_from_non_project_dir() {
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
        .context("proj-dir-create-1", "created `inst1`")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .arg("--project-dir=tests/proj/project1")
        .assert()
        .context("proj-dir-project-info-no", "not initialied")
        .code(1)
        .stderr(predicates::str::contains("is not initialized"));

    Command::new("edgedb")
        .arg("project").arg("init").arg("--link")
        .arg("--server-instance=inst1")
        .arg("--non-interactive")
        .arg("--project-dir=tests/proj/project1")
        .assert()
        .context("proj-dir-project-link", "linked `inst1` to project project1")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .arg("--project-dir=tests/proj/project1")
        .assert()
        .context("proj-dir-project-info", "instance-name == inst1")
        .success()
        .stdout(predicates::ord::eq("inst1\n"));

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project1")
        .assert()
        .context("proj-dir-query-1", "query of project")
        .success();

    Command::new("edgedb").arg("project").arg("init")
        .arg("--non-interactive")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-project-init", "init project2")
        .success();

    Command::new("edgedb")
        .arg("project").arg("info").arg("--instance-name")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-project-info", "instance-name == project2")
        .success()
        .stdout(predicates::ord::eq("project2\n"));

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("proj-dir-query-2", "query of project2")
        .success();

    Command::new("edgedb").arg("project").arg("upgrade")
        .arg("--force")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-project-upgrade", "upgrade project")
        .success();

    Command::new("edgedb")
        .arg("query").arg("SELECT 1")
        .current_dir("tests/proj/project2")
        .assert()
        .context("proj-dir-query-3", "query after upgrade")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2")
        .arg("--non-interactive")
        .assert()
        .context("proj-dir-destroy-2-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("inst1")
        .arg("--non-interactive")
        .assert()
        .context("proj-dir-destroy-1-no", "should warn")
        .code(2);

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project1")
        .arg("--non-interactive")
        .assert()
        .context("proj-dir-destroy-1-non-exist", "it's project name, not instance name")
        .code(8); // instance not found

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("proj-dir-instance-list-1", "list two instances")
        .success()
        .stdout(predicates::str::contains("inst1"))
        .stdout(predicates::str::contains("project2"));

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("project2")
        .arg("--force")
        .assert()
        .context("proj-dir-destroy-2", "should destroy")
        .success();

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("proj-dir-instance-list-2", "list once instance")
        .success()
        .stdout(predicates::str::contains("inst1"))
        .stdout(predicates::str::contains("project2").not());


    Command::new("edgedb")
        .arg("project").arg("unlink").arg("-D").arg("--non-interactive")
        .arg("--project-dir=tests/proj/project1")
        .assert()
        .context("proj-dir-destroy-1", "should unlink and destroy project")
        .success();

    Command::new("edgedb").arg("instance").arg("list")
        .assert()
        .context("proj-dir-instance-list-3", "list no instances")
        .success()
        .stdout(predicates::str::contains("inst1").not())
        .stdout(predicates::str::contains("project2").not());

    Command::new("edgedb").arg("project").arg("init")
        .arg("--non-interactive")
        .arg("--server-start-conf=manual")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-project-init-manual", "init project2 manual")
        .success();

    Command::new("edgedb").arg("project").arg("upgrade")
        .arg("--to-latest").arg("--force")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-project-upgrade-manual", "upgrade manual project")
        .success();

    Command::new("edgedb").arg("instance").arg("status").arg("project2")
        .arg("--extended")
        .assert()
        .context("proj-dir-instance-status", "show extended status")
        .code(3);

    Command::new("edgedb").arg("instance").arg("revert").arg("project2")
        .arg("--no-confirm")
        .assert()
        .context("proj-dir-project-revert-manual", "revert manual project")
        .success();

    Command::new("edgedb")
        .arg("project").arg("unlink").arg("-D").arg("--non-interactive")
        .arg("--project-dir=tests/proj/project2")
        .assert()
        .context("proj-dir-destroy-2", "should unlink and destroy project")
        .success();
}
