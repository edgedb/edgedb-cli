use anyhow;

use async_std::task;

use std::env;

use crate::options::Options;

mod client;
mod commands;
mod completion;
mod error_display;
mod highlight;
mod interactive;
mod non_interactive;
mod options;
mod outputs;
mod print;
mod prompt;
mod reader;
mod repl;
mod server;
mod server_params;
mod statement;
mod variables;


fn main() -> Result<(), anyhow::Error> {
    // If a crash happens we want the backtrace to be printed by default
    // to ease bug reporting and troubleshooting.
    // TODO: consider removing this once EdgeDB reaches 1.0 stable.
    env::set_var("RUST_BACKTRACE", "1");

    let opt = Options::from_args_and_env();
    env_logger::init_from_env(env_logger::Env::default()
        .default_filter_or("warn"));
    if opt.subcommand.is_some() {
        commands::cli::main(opt)
    } else {
        if opt.interactive {
            interactive::main(opt)
        } else {
            task::block_on(non_interactive::main(opt))
        }
    }
}
