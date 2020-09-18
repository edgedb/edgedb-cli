use async_std::task;
use edgedb_client::Builder;

use crate::platform::home_dir;


pub fn get_connector(name: &str) -> anyhow::Result<Builder> {
    task::block_on(Builder::read_credentials(
        home_dir()?.join(".edgedb").join("credentials")
        .join(format!("{}.json", name))
    ))
}
