use assert_cmd::Command;

use crate::util::OutputExt;
use crate::SERVER;

#[test]
fn with_comment() {
    SERVER
        .admin_cmd()
        .write_stdin("SELECT 1; # comment")
        .assert()
        .success();
}

#[test]
fn deprecated_unix_host() {
    SERVER
        .admin_cmd_deprecated()
        .write_stdin("SELECT 1")
        .assert()
        .success();
}

#[test]
fn stdin_password() {
    SERVER
        .admin_cmd()
        .arg("--password-from-stdin")
        .write_stdin("password\n")
        .assert()
        .success();
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
    SERVER
        .admin_cmd()
        .arg("list")
        .arg("indexes")
        .assert()
        .success();
}

#[test]
fn database_create_wipe_drop() {
    SERVER
        .admin_cmd()
        .arg("database")
        .arg("create")
        .arg("test_create_wipe_drop")
        .assert()
        .context("create", "create new database")
        .success();

    SERVER
        .admin_cmd()
        .arg("query")
        .arg("--database=test_create_wipe_drop")
        .arg("CREATE TYPE Type1")
        .arg("INSERT Type1")
        .assert()
        .context("add-data", "add some data to the new database")
        .success();

    SERVER
        .admin_cmd()
        .arg("query")
        .arg("--database=test_create_wipe_drop")
        .arg("SELECT Type1")
        .assert()
        .context("check-data", "check that added data is still there")
        .stdout(predicates::str::contains(r#"{"id":"#))
        .success();

    SERVER
        .admin_cmd()
        .arg("database")
        .arg("wipe")
        .arg("--database=test_create_wipe_drop")
        .arg("--non-interactive")
        .assert()
        .context("wipe", "wipe the data out")
        .success();

    SERVER
        .admin_cmd()
        .arg("query")
        .arg("--database=test_create_wipe_drop")
        .arg("CREATE TYPE Type1")
        .assert()
        .context("create-again", "check that type can be created again")
        .success();

    SERVER
        .admin_cmd()
        .arg("--database=test_create_wipe_drop")
        .arg("database")
        .arg("drop")
        .arg("test_create_wipe_drop")
        .arg("--non-interactive")
        .assert()
        .context("drop-same", "cannot drop the same database")
        .failure();

    SERVER
        .admin_cmd()
        .arg("database")
        .arg("drop")
        .arg("test_create_wipe_drop")
        .arg("--non-interactive")
        .assert()
        .context("drop", "drop successfully")
        .success();

    SERVER
        .admin_cmd()
        .arg("query")
        .arg("--database=test_create_wipe_drop")
        .arg("SELECT Type1")
        .assert()
        .context("select-again", "make sure that database is not there")
        .failure();
}

#[test]
fn branch_commands() {
    let default_branch = SERVER.default_branch();

    SERVER
        .admin_cmd()
        .arg("branch")
        .arg("current")
        .arg("--plain")
        .assert()
        .context("current", "should print the default branch")
        .success()
        .stdout(predicates::str::contains(default_branch));

    // create --empty
    SERVER
        .admin_cmd()
        .arg("branch")
        .arg("create")
        .arg("--empty")
        .arg("test_branch_1")
        .assert()
        .context("create", "new empty branch")
        .success();

    // create --from
    SERVER
        .admin_cmd()
        .arg("branch")
        .arg("create")
        .arg("test_branch_2")
        .arg("--from")
        .arg("test_branch_1")
        .assert()
        .context("create", "new branch from test_branch_1")
        .success();

    // create without --from and --empty should use the current database
    SERVER
        .admin_cmd()
        .arg("branch")
        .arg("create")
        .arg("test_branch_3")
        .assert()
        .success();

    SERVER
        .admin_cmd()
        .arg("--branch")
        .arg("test_branch_1")
        .arg("branch")
        .arg("current")
        .arg("--plain")
        .assert()
        .context("create", "check the current branch")
        .success()
        .stdout(predicates::str::contains("test_branch_1"));

    // switch
    SERVER
        .admin_cmd()
        .arg("branch")
        .arg("switch")
        .arg("test_branch_2")
        .assert()
        .context("switch", "without specifying an instance")
        .failure()
        .stderr(predicates::str::contains(
            "Cannot switch branches without specifying the instance",
        ));

    // switch requires instance name, so let's link the test instance
    let instance_name = SERVER.ensure_instance_linked();

    crate::edgedb_cli_cmd()
        .arg("--instance")
        .arg(instance_name)
        .arg("branch")
        .arg("current")
        .arg("--plain")
        .assert()
        .context("current", "with --instance")
        .success()
        .stdout(predicates::str::contains(default_branch));

    crate::edgedb_cli_cmd()
        .arg("--instance")
        .arg(instance_name)
        .arg("branch")
        .arg("switch")
        .arg("test_branch_2")
        .assert()
        .context("switch", "to test_branch_2 with --instance")
        .success();

    crate::edgedb_cli_cmd()
        .arg("--instance")
        .arg(instance_name)
        .arg("branch")
        .arg("current")
        .arg("--plain")
        .assert()
        .context("current", "with --instance")
        .success()
        .stdout(predicates::str::contains("test_branch_2"));

    // TODO: test how this works in projects
}

#[test]
fn hash_password() {
    crate::edgedb_cli_cmd()
        .arg("hash-password")
        .arg("password1234")
        .assert()
        .context("hash-password", "basic usage")
        .success()
        .stdout(predicates::str::starts_with("SCRAM-SHA-256$"));
}
