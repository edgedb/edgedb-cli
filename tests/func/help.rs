use assert_cmd::Command;

#[test]
fn help_connect() {
    let cmd = Command::cargo_bin("edgedb")
              .expect("binary found")
              .arg("--help-connect")
              .assert()
              .success();

    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();

    assert!(out.contains("-H, --host"));
}

#[test]
fn help_no_extended_connect_help() {
    let cmd = Command::cargo_bin("edgedb")
              .expect("binary found")
              .arg("--help")
              .assert()
              .success();

    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();

    assert!(!out.contains("-H, --host"));
}
