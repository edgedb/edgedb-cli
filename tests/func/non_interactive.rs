use crate::SERVER;


#[test]
fn with_comment() {
    SERVER.admin_cmd()
        .write_stdin("SELECT 1; # comment")
        .assert().success();
}

#[test]
fn stdin_password() {
    SERVER.admin_cmd()
        .arg("--password-from-stdin")
        .write_stdin("password\n")
        .assert().success();
}
