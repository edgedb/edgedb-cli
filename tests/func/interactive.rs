use std::error::Error;
use crate::{Config, SERVER};


#[test]
fn simple_query() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("main>")?;
    cmd.send_line("SELECT 'abc'++'def';\n")?;
    cmd.exp_string("abcdef")?;
    cmd.exp_string("main>")?;
    cmd.send_line(" SELECT 'xy'++'z';\n")?;
    cmd.exp_string("xyz")?;
    Ok(())
}

#[test]
fn two_queries() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("main>")?;
    cmd.send_line("SELECT 'AB'++'C'; SELECT 'XY'++'Z';\n")?;
    cmd.exp_string("ABC")?;
    cmd.exp_string("XYZ")?;
    Ok(())
}

#[test]
fn test_switch_database() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("main>")?;
    cmd.send_line("create empty branch _test_switch_asdf;")?;
    cmd.send_line("\\c _test_switch_asdf")?;
    cmd.exp_string("_test_switch_asdf>")?;
    cmd.send_line("\\c main")?;
    cmd.exp_string("main>")?;
    cmd.send_line("drop branch _test_switch_asdf;")?;
    Ok(())
}

#[test]
fn create_report() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("main>")?;
    cmd.send_line("CREATE TYPE default::Type1;\n")?;
    cmd.exp_string("OK: CREATE")?;
    Ok(())
}

#[test]
fn configured_limit() -> Result<(), Box<dyn Error>> {
    let config = Config::new(r###"
[shell]
limit = 2
"###);
    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.env("XDG_CONFIG_HOME", config.path());
    });
    cmd.exp_string("main>")?;
    cmd.send_line("SELECT {'abc', 'def', 'fgh'};\n")?;
    cmd.exp_string("...")?;

    let config = Config::new(r###"
[shell]
limit = 3
"###);
    let mut cmd = SERVER.custom_interactive(|cmd| {
        cmd.env("XDG_CONFIG_HOME", config.path());
    });
    cmd.exp_string("main>")?;
    cmd.send_line("SELECT {'abc', 'def', 'fgh'};\n")?;
    cmd.exp_string("{")?;
    cmd.exp_string("fgh")?;

    Ok(())
}
