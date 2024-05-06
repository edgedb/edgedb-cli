use crate::SERVER;

#[test]
fn non_interactive_link() {
    let instance_name = SERVER.ensure_instance_linked();

    crate::edgedb_cli_cmd()
        .arg(format!("-I{instance_name}"))
        .arg("query")
        .arg("SELECT 7*8")
        .assert()
        .success()
        .stdout("56\n");
}

#[test]
fn link_requires_conn() {
    crate::edgedb_cli_cmd()
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
