use edgedb_cli_derive::{EdbClap};

use crate::options::CloudOptions;


#[derive(EdbClap, Debug, Clone)]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Authenticate to the EdgeDB Cloud and remember the access token locally
    #[edb(inherit(CloudOptions))]
    Login(Login),
    /// Forget the stored access token
    Logout(Logout),
}

#[derive(EdbClap, Debug, Clone)]
pub struct Login {
}

#[derive(EdbClap, Debug, Clone)]
pub struct Logout {
    #[clap(long)]
    pub all_profiles: bool,
}
