use crate::commands::Options;
use crate::commands::parser::CreateMigration;
use crate::client::Connection;

use crate::migrations::context::Context;


pub async fn create(cli: &mut Connection, options: &Options,
    create: &CreateMigration)
    -> Result<(), anyhow::Error>
{
    let ctx = Context::from_config(&create.cfg);
    todo!();
}
