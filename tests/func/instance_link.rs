use assert_cmd::Command;
use crate::SERVER;

#[test]
fn non_interactive_link() {
    Command::cargo_bin("edgedb").expect("binary found")
        .env("CLICOLOR", "0")
        .arg("--no-cli-update-check")
        .arg("instance")
        .arg("link")
        .arg("--port").arg(SERVER.port.to_string())
        .arg("--non-interactive")
        .arg("--trust-tls-cert")
        .arg("--overwrite")
        .arg("--quiet")
        .arg("_test_inst")
        .assert()
        .success();
    Command::cargo_bin("edgedb").expect("binary found")
        .env("CLICOLOR", "0")
        .env("RUST_LOG", "debug")
        .arg("--no-cli-update-check")
        .arg("-I_test_inst")
        .arg("query")
        .arg("SELECT 7*8")
        .assert()
        .success()
        .stdout("56\n");
}

#[test]
fn link_requires_conn() {
    Command::cargo_bin("edgedb").expect("binary found")
        .env("CLICOLOR", "0")
        .arg("--no-cli-update-check")
        .arg("instance")
        .arg("link")
        .arg("--non-interactive")
        .arg("--trust-tls-cert")
        .arg("--overwrite")
        .arg("--quiet")
        .arg("_test_inst")
        .assert()
        .code(1)
        .stderr(predicates::str::contains("no connection options"));
}
