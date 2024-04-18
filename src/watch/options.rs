use crate::options::ConnectionOptions;

#[derive(clap::Args, Debug, Clone)]
pub struct WatchCommand {
    #[command(flatten)]
    pub conn: ConnectionOptions,

    /// Print DDLs applied to the schema.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}
