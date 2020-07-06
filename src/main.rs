use std::env;
use std::path::Path;
use std::process::exit;

use async_std::task;
use clap::Clap;

use crate::options::Options;

mod client;
mod commands;
mod completion;
mod error_display;
mod highlight;
mod interactive;
mod log_levels;
mod non_interactive;
mod options;
mod outputs;
mod print;
mod process;
mod prompt;
mod platform;
mod reader;
mod repl;
mod self_install;
mod server;
mod server_params;
mod statement;
mod table;
mod variables;

fn main() {
    match _main() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {:#}", e);
            exit(1);
        }
    }
}

fn _main() -> Result<(), anyhow::Error> {
    // If a crash happens we want the backtrace to be printed by default
    // to ease bug reporting and troubleshooting.
    // TODO: consider removing this once EdgeDB reaches 1.0 stable.
    env::set_var("RUST_BACKTRACE", "1");

    if let Some(arg0) = std::env::args_os().next() {
        if let Some(exe_name) = Path::new(&arg0).file_name() {
            if exe_name.to_string_lossy().contains("-init") {
                let opt = self_install::SelfInstall::parse();
                return self_install::main(&opt);
            }
        }
    }

    let opt = Options::from_args_and_env();

    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn")
    );
    log_levels::init(&mut builder, &opt);
    builder.init();

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
