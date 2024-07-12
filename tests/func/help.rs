#[test]
fn help_connect() {
    let cmd = crate::edgedb_cli_cmd()
        .arg("--help-connect")
        .assert()
        .success();

    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();

    assert!(out.contains("--host"));
}

#[test]
fn help_no_extended_connect_help() {
    let cmd = crate::edgedb_cli_cmd().arg("--help").assert().success();

    let out = String::from_utf8(cmd.get_output().stdout.clone()).unwrap();

    assert!(!out.contains("-H, --host"));
}
