use crate::SERVER;


#[test]
fn with_comment() {
    SERVER.admin_cmd()
        .write_stdin("SELECT 1; # comment")
        .assert().success();
}
