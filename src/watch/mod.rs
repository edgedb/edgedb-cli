mod fs_watcher;
mod main;
mod files;

pub use fs_watcher::FsWatcher;
pub use main::run;

#[derive(clap::Args, Debug, Clone)]
pub struct WatchCommand {
    #[command(flatten)]
    pub conn: crate::options::ConnectionOptions,

    /// Watch files and execute scripts defined in gel.toml
    #[arg(short = 'f', long)]
    pub files: bool,

    /// Print DDLs applied to the schema.
    #[arg(short = 'v', long)]
    pub verbose: bool,
}
