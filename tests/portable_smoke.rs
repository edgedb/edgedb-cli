#![cfg_attr(not(feature="portable_tests"), allow(dead_code))]

use assert_cmd::Command;
mod util;

use util::*;


#[cfg(feature="portable_tests")]
#[test]
fn install() {
    Command::new("edgedb")
        .arg("--version")
        .assert()
        .context("version", "command-line version option")
        .success()
        .stdout(predicates::str::contains(
            concat!("EdgeDB CLI ", env!("CARGO_PKG_VERSION"))));

    // only nightly so far
    Command::new("edgedb")
        .arg("instance").arg("create").arg("inst1").arg("--nightly")
        .assert()
        .context("create-1", "created `inst1`")
        .success();

    // TODO(tailhook) check output somehow
    Command::new("edgedb")
        .arg("server").arg("list-versions")
        .assert()
        .context("list-versions", "list versions of the server")
        .success();

    Command::new("edgedb")
        .arg("server").arg("list-versions")
        .arg("--installed-only")
        .assert()
        .context("list-versions-installed", "")
        .success();

    Command::new("edgedb")
        .arg("server").arg("list-versions")
        .arg("--json")
        .assert()
        .context("list-versions-json", "")
        .success();

    Command::new("edgedb")
        .arg("server").arg("list-versions")
        .arg("--json").arg("--installed")
        .assert()
        .context("list-versions-json-installed", "")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("logs").arg("inst1")
        .assert()
        .context("log-1-0", "logs of `inst1`")
        .success();

    Command::new("edgedb")
        .arg("--admin").arg("--instance").arg("inst1")
        .arg("query").arg("SELECT 1")
        .env("RUST_LOG", "debug")
        .assert()
        .context("query-1", "query `inst1` first time")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("status").arg("inst1")
        .assert()
        .context("status-1", "status `inst1` first time")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("restart").arg("inst1")
        .assert()
        .context("restart-1", "restart `inst1`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("logs").arg("inst1")
        .assert()
        .context("log-1-1", "logs of `inst1`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("status").arg("inst1")
        .assert()
        .context("status-1-1", "status `inst1` after restart")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("stop").arg("inst1")
        .env("RUST_LOG", "warn,edgedb::process=debug")
        .assert()
        .context("stop-1", "stop `inst1`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("status").arg("inst1")
        .assert()
        .context("status-1-2", "status `inst1` after stop")
        .code(3);

    Command::new("edgedb")
        .arg("instance").arg("create").arg("second").arg("--nightly")
            // .arg("--version=1-beta3")  TODO(tailhook)
        .assert()
        .context("create-2", "create `second`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("start").arg("second")
        .assert()
        .context("start-2", "start `second`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("start").arg("inst1")
        .assert()
        .context("start-1-3", "start `inst1` again")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("status").arg("second")
        .assert()
        .context("status-2", "status `second`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("logs").arg("inst1")
        .assert()
        .context("log-1-2", "logs of `inst1`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("logs").arg("second")
        .assert()
        .context("log-2", "logs of `second`")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("status").arg("inst1")
        .assert()
        .context("status-1-4", "status of `inst1`")
        .success();

    /*
    * TODO
    Command::new("edgedb")
        .arg("instance").arg("upgrade").arg("inst1")
        .arg("--to-latest").arg("--force")
        .assert()
        .context("upgrade-1", "force upgrade `inst1` to latest")
        .success();

    Command::new("edgedb")
        .arg("--admin").arg("--instance").arg("inst1")
        .arg("query").arg("SELECT 1")
        .assert()
        .context("query-1-2", "query `inst1` after upgrade")
        .success();

    Command::new("edgedb")
        .arg("instance").arg("destroy").arg("second")
        .assert()
        .context("destroy-2", "destroy `second` instance")
        .success();


    Command::new(&edgedb)
        .arg("server").arg("uninstall").arg("--version=1-beta3")
        .assert()
        .context("uninstall-2", "uninstall old version")
        .success();
    Command::new(&edgedb)
        .arg("server").arg("list-versions")
        .arg("--installed-only").arg("--column=major-version")
        .assert()
        .success()
        .context("list-2", "list after uninstall")
        .stdout(predicates::str::contains("1-beta2").not());
    */

    Command::new("edgedb")
        .arg("--admin").arg("--instance").arg("inst1")
        .arg("query").arg("SELECT 1")
        .assert()
        .context("query-1a", "late query of `inst1`")
        .success();
}
