mod fs_watcher;
mod main;

pub use fs_watcher::FsWatcher;
pub use main::run;

#[derive(clap::Args, Debug, Clone)]
pub struct WatchCommand {
    #[command(flatten)]
    pub conn: crate::options::ConnectionOptions,

    /// Print DDLs applied to the schema.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}
