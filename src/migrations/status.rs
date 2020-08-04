use crate::commands::Options;
use crate::commands::parser::ShowStatus;
use crate::client::Connection;

pub async fn status(cli: &mut Connection, options: &Options,
    status: &ShowStatus)
    -> Result<(), anyhow::Error>
{
    todo!();
}
