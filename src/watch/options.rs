use edgedb_cli_derive::{EdbClap};


#[derive(EdbClap, Debug, Clone)]
pub struct WatchCommand {
    /// Print DDLs applied to the schema
    #[clap(short='v', long)]
    pub verbose: bool,
}
