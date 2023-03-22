use crate::options::CloudOptions;
use crate::cloud::options::CloudCommand;
use crate::cloud::auth;
use crate::cloud::secret_keys;

pub fn cloud_main(cmd: &CloudCommand, options: &CloudOptions) -> anyhow::Result<()> {
    use crate::cloud::options::Command::*;

    match &cmd.subcommand {
        Login(c) => {
            auth::login(c, options)
        }
        Logout(c) => {
            auth::logout(c, options)
        }
        SecretKey(c) => {
            secret_keys::main(c, options)
        }
    }
}
