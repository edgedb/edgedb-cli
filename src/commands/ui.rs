use crate::options::Options;
use crate::print;
use crate::commands::ExitCode;

pub fn show_ui(options: &Options) -> anyhow::Result<()> {
    let connector = options.create_connector()?;
    let builder = connector.get()?;
    let url = format!("http://{}:{}/admin", builder.get_host(), builder.get_port());
    if open::that(&url).is_ok() {
        Ok(())
    } else {
        print::error("Cannot launch browser, please visit URL:");
        print::echo!("  {}", url);
        Err(ExitCode::new(1).into())
    }
}
