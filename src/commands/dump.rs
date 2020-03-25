use std::path::Path;

use crate::commands::Options;
use crate::client::Client;


pub async fn dump<'x>(cli: &mut Client<'x>, options: &Options, filename: &Path)
    -> Result<(), anyhow::Error>
{
    todo!();
}
