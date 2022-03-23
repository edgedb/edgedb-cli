use async_std::task;

use crate::cloud::options::CloudCommand;
use crate::cloud::auth;


pub fn cloud_main(cmd: &CloudCommand) -> anyhow::Result<()> {
    use crate::cloud::options::Command::*;

    match &cmd.subcommand {
        Login(c) => {
            task::block_on(auth::login(c))
        }
        Logout(c) => {
            task::block_on(auth::logout(c))
        }
    }
}
