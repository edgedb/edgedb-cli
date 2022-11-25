use async_std::task;

use crate::options::CloudOptions;
use crate::cloud::options::CloudCommand;
use crate::cloud::auth;


pub fn cloud_main(cmd: &CloudCommand, options: &CloudOptions) -> anyhow::Result<()> {
    use crate::cloud::options::Command::*;

    match &cmd.subcommand {
        Login(c) => {
            task::block_on(auth::login(c, options))
        }
        Logout(c) => {
            auth::logout(c, options)
        }
    }
}
