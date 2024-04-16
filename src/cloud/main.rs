use crate::cloud::auth;
use crate::cloud::options::CloudCommand;
use crate::cloud::secret_keys;
use crate::options::CloudOptions;

pub fn cloud_main(cmd: &CloudCommand, options: &CloudOptions) -> anyhow::Result<()> {
    use crate::cloud::options::Command::*;

    match &cmd.subcommand {
        Login(c) => auth::login(c, options),
        Logout(c) => auth::logout(c, options),
        SecretKey(c) => secret_keys::main(c, options),
    }
}
