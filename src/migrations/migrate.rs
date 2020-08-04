use crate::commands::Options;
use crate::commands::parser::Migrate;
use crate::client::Connection;


pub async fn migrate(cli: &mut Connection, options: &Options,
    migrate: &Migrate)
    -> Result<(), anyhow::Error>
{
    todo!();
}
