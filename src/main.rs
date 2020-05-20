use anyhow;

use async_std::task;
use async_std::sync::{channel};

use std::env;

use crate::options::Options;

mod client;
mod commands;
mod completion;
mod highlight;
mod options;
mod print;
mod prompt;
mod reader;
mod repl;
mod server;
mod server_params;
mod statement;
mod variables;
mod error_display;


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
            interactive_main(opt)
        } else {
            non_interactive_main(opt)
        }
    }
}

fn interactive_main(options: Options) -> Result<(), anyhow::Error> {
    let (control_wr, control_rd) = channel(1);
    let (repl_wr, repl_rd) = channel(1);
    let state = repl::State {
        control: control_wr,
        data: repl_rd,
        print: print::Config::new()
            .max_items(100)
            .colors(atty::is(atty::Stream::Stdout))
            .clone(),
        verbose_errors: false,
        last_error: None,
        database: options.database.clone(),
        implicit_limit: Some(100),
        output_mode: options.output_mode,
        input_mode: repl::InputMode::Emacs,
        history_limit: 100,
    };
    let handle = task::spawn(client::interactive_main(options, state));
    prompt::main(repl_wr, control_rd)?;
    task::block_on(handle)?;
    Ok(())
}

fn non_interactive_main(options: Options) -> Result<(), anyhow::Error> {
    task::block_on(client::non_interactive_main(options))
}

