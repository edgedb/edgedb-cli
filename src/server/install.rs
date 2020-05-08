use async_std::task;

use crate::server::options::Install;
use crate::server::remote;

const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";




pub fn install(_options: &Install) -> Result<(), anyhow::Error> {
    let key = task::block_on(remote::get_string(KEY_FILE_URL,
        "downloading key file"))?;
    todo!();
}
