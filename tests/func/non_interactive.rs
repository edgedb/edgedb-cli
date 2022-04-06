use crate::SERVER;
use assert_cmd::Command;


#[test]
fn with_comment() {
    SERVER.admin_cmd()
        .write_stdin("SELECT 1; # comment")
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
