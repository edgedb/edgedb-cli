use std::error::Error;
use crate::SERVER;


#[test]
fn simple_query() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("edgedb>")?;
    cmd.send_line("SELECT 'abc'++'def';\n")?;
    cmd.exp_string("abcdef")?;
    cmd.exp_string("edgedb>")?;
    cmd.send_line(" SELECT 'xy'++'z';\n")?;
    cmd.exp_string("xyz")?;
    Ok(())
}

#[test]
fn two_queries() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("edgedb>")?;
    cmd.send_line("SELECT 'AB'++'C'; SELECT 'XY'++'Z';\n")?;
    cmd.exp_string("ABC")?;
    cmd.exp_string("XYZ")?;
    Ok(())
}

#[test]
fn create_report() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("edgedb>")?;
    cmd.send_line("CREATE TYPE default::Type1;\n")?;
    cmd.exp_string("OK: CREATE")?;
    Ok(())
}
