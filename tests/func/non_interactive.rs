use crate::SERVER;
use assert_cmd::Command;


#[test]
fn with_comment() {
    SERVER.admin_cmd()
        .write_stdin("SELECT 1; # comment")
        .assert().success();
}

#[test]
fn deprecated_unix_host() {
    SERVER.admin_cmd_deprecated()
        .write_stdin("SELECT 1")
        .assert().success();
}

#[test]
fn stdin_password() {
    SERVER.admin_cmd()
        .arg("--password-from-stdin")
        .write_stdin("password\n")
        .assert().success();
}

#[test]
fn strict_version_check() {
    Command::cargo_bin("edgedb")
        .expect("binary found")
        .env("EDGEDB_RUN_VERSION_CHECK", "strict")
        .arg("info")
        .assert()
        .success();
}

#[test]
fn list_indexes() {
    SERVER.admin_cmd()
        .arg("list")
        .arg("indexes")
        .assert().success();
}
