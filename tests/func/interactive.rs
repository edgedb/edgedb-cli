use crate::SERVER;
use std::error::Error;

#[test]
fn simple_query() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("edgedb>")?;
    cmd.send_line("SELECT 1+2;\n")?;
    cmd.exp_string("{\u{1b}[38;5;2m3\u{1b}[0m}\r\n")?;
    cmd.exp_string("edgedb>")?;
    cmd.send_line(" SELECT 2+3;\n")?;
    cmd.exp_string("{\u{1b}[38;5;2m5\u{1b}[0m}\r\n")?;
    Ok(())
}

#[test]
fn two_queries() -> Result<(), Box<dyn Error>> {
    let mut cmd = SERVER.admin_interactive();
    cmd.exp_string("edgedb>")?;
    cmd.send_line("SELECT 1+2; SELECT 2+3;\n")?;
    cmd.exp_string("{\u{1b}[38;5;2m3\u{1b}[0m}\r\n")?;
    cmd.exp_string("{\u{1b}[38;5;2m5\u{1b}[0m}\r\n")?;
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
