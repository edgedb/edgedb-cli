use edgedb_cli_derive::{EdbClap};


#[derive(EdbClap, Debug, Clone)]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Authenticate to the EdgeDB Cloud and remember the access token locally
    Login(Login),
    /// Forget the stored access token
    Logout(Logout),
}

#[derive(EdbClap, Debug, Clone)]
pub struct Login {
    #[edb(hide=true)]
    pub cloud_base_url: Option<String>
}

#[derive(EdbClap, Debug, Clone)]
pub struct Logout {

}
