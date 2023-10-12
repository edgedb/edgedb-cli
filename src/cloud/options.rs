use crate::options::CloudOptions;

#[derive(clap::Args, Debug, Clone)]
pub struct CloudCommand {
    #[command(flatten)]
    pub cloud: CloudOptions,

    #[command(subcommand)]
    pub subcommand: Command,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum Command {
    /// Authenticate to EdgeDB Cloud and remember secret key locally
    Login(Login),
    /// Forget the stored access token
    Logout(Logout),
    /// Secret key management
    #[command(name = "secretkey")]
    SecretKey(SecretKeyCommand),
}

#[derive(clap::Args, Debug, Clone)]
pub struct Login {
}

#[derive(clap::Args, Debug, Clone)]
pub struct Logout {
    /// Log out from all Cloud profiles
    #[arg(long)]
    #[arg(hide=true)]
    pub all_profiles: bool,

    /// Force log out from all profiles, even if linked to a project
    #[arg(long)]
    pub force: bool,

    /// Do not ask questions, assume user wants to log out of all profiles not
    /// linked to a project
    #[arg(long)]
    pub non_interactive: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct SecretKeyCommand {
    #[command(subcommand)]
    pub subcommand: SecretKeySubCommand
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum SecretKeySubCommand {
    /// List existing secret keys.
    List(ListSecretKeys),
    /// Create a new secret key.
    Create(CreateSecretKey),
    /// Revoke a secret key.
    Revoke(RevokeSecretKey),
}

#[derive(clap::Args, Debug, Clone)]
pub struct ListSecretKeys {
    /// Output results as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct CreateSecretKey {
    /// Output results as JSON
    #[arg(long)]
    pub json: bool,
    /// Friendly key name
    #[arg(short='n', long)]
    pub name: Option<String>,
    /// Long key description
    #[arg(long)]
    pub description: Option<String>,

    /// Key expiration in duration units (e.g. "1 hour 30 minutes").
    /// Does not expire if set to `never`.
    #[arg(long, value_name = "<duration> | \"never\"")]
    pub expires: Option<String>,

    /// Comma-separated list of key scopes.
    /// Mutually exclusive with `--inherit-scopes`.
    #[arg(
        long,
        group = "key_scopes",
        conflicts_with = "inherit_scopes",
        value_delimiter = ','
    )]
    pub scopes: Option<Vec<String>>,
    /// Inherit key scopes from the currently used key.  Mutually exclusive
    /// with `--scopes`.
    #[arg(long, group = "key_scopes", conflicts_with = "scopes")]
    pub inherit_scopes: bool,

    /// Do not ask questions, assume default answers to all inputs
    /// that have a default.  Requires key TTL and scopes to be explicitly
    /// specified via `--ttl` or `--no-expiration`, and `--scopes` or
    /// `--inherit-scopes`.
    #[arg(short='y', long)]
    #[arg(requires_ifs(
        ["expires", "key_scopes"].iter().map(
            |id| (clap::builder::ArgPredicate::IsPresent, id)
        )
    ))]
    pub non_interactive: bool,
}

#[derive(clap::Args, Debug, Clone)]
pub struct RevokeSecretKey {
    /// Output results as JSON
    #[arg(long)]
    pub json: bool,
    /// Id of secret key to revoke
    #[arg(long)]
    pub secret_key_id: String,
    /// Revoke key without asking for confirmation
    #[arg(short='y', long)]
    pub non_interactive: bool,
}
