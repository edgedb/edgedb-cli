use edgedb_cli_derive::EdbClap;

use crate::options::CloudOptions;


#[derive(EdbClap, Debug, Clone)]
pub struct CloudCommand {
    #[clap(subcommand)]
    pub subcommand: Command,
}

#[derive(EdbClap, Clone, Debug)]
pub enum Command {
    /// Authenticate to EdgeDB Cloud and remember secret key locally
    #[edb(inherit(CloudOptions))]
    Login(Login),
    /// Forget the stored access token
    Logout(Logout),
    /// Secret key management
    #[clap(name = "secretkey")]
    SecretKey(SecretKeyCommand),
}

#[derive(EdbClap, Debug, Clone)]
pub struct Login {
}

#[derive(EdbClap, Debug, Clone)]
pub struct Logout {
    /// Log out from all Cloud profiles
    #[clap(long)]
    #[clap(hide=true)]
    pub all_profiles: bool,

    /// Force log out from all profiles, even if linked to a project
    #[clap(long)]
    pub force: bool,

    /// Do not ask questions, assume user wants to log out of all profiles not
    /// linked to a project
    #[clap(long)]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct SecretKeyCommand {
    #[clap(subcommand)]
    pub subcommand: SecretKeySubCommand
}

#[derive(EdbClap, Clone, Debug)]
pub enum SecretKeySubCommand {
    /// List existing secret keys.
    #[edb(inherit(CloudOptions))]
    List(ListSecretKeys),
    /// Create a new secret key.
    #[edb(inherit(CloudOptions))]
    Create(CreateSecretKey),
    /// Revoke a secret key.
    #[edb(inherit(CloudOptions))]
    Revoke(RevokeSecretKey),
}

#[derive(EdbClap, Debug, Clone)]
pub struct ListSecretKeys {
    /// Output results as JSON
    #[clap(long)]
    pub json: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct CreateSecretKey {
    /// Output results as JSON
    #[clap(long)]
    pub json: bool,
    /// Friendly key name
    #[clap(short='n', long)]
    pub name: Option<String>,
    /// Long key description
    #[clap(long)]
    pub description: Option<String>,

    /// Key expiration in duration units (e.g. "1 hour 30 minutes").
    /// Does not expire if set to `never`.
    #[clap(long, value_name = "<duration> | \"never\"")]
    pub expires: Option<String>,

    /// Comma-separated list of key scopes.
    /// Mutually exclusive with `--inherit-scopes`.
    #[clap(
        long,
        group = "key_scopes",
        conflicts_with = "inherit_scopes",
        value_delimiter = ','
    )]
    pub scopes: Option<Vec<String>>,
    /// Inherit key scopes from the currently used key.  Mutually exclusive
    /// with `--scopes`.
    #[clap(long, group = "key_scopes", conflicts_with = "scopes")]
    pub inherit_scopes: bool,

    /// Do not ask questions, assume default answers to all inputs
    /// that have a default.  Requires key TTL and scopes to be explicitly
    /// specified via `--ttl` or `--no-expiration`, and `--scopes` or
    /// `--inherit-scopes`.
    #[clap(short='y', long)]
    #[clap(requires_ifs(
        ["expires", "key_scopes"].iter().map(
            |id| (clap::builder::ArgPredicate::IsPresent, id)
        )
    ))]
    pub non_interactive: bool,
}

#[derive(EdbClap, Debug, Clone)]
pub struct RevokeSecretKey {
    /// Output results as JSON
    #[clap(long)]
    pub json: bool,
    /// Id of secret key to revoke
    #[clap(long)]
    pub secret_key_id: String,
    /// Revoke key without asking for confirmation
    #[clap(short='y', long)]
    pub non_interactive: bool,
}
