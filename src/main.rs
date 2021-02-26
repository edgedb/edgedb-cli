// We don't need to hunt of unused imports on windows, as they are harmless
#![cfg_attr(windows, allow(unused_imports))]
#![type_length_limit="4194304"]

use std::env;
use std::path::Path;
use std::process::exit;

use async_std::task;
use clap::Clap;

use crate::options::Options;

mod async_util;
mod bug;
mod commands;
mod completion;
mod connect;
mod credentials;
mod error_display;
mod format;
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
mod repl;
mod self_install;
mod self_upgrade;
mod server;
mod statement;
mod table;
mod variables;
mod version_check;
mod migrations;

fn main() {
    match _main() {
        Ok(()) => {}
        Err(e) => {
            if let Some(e) = e.downcast_ref::<commands::ExitCode>() {
                e.exit();
            }
            if let Some(e) = e.downcast_ref::<bug::Bug>() {
                eprintln!("edgedb error: {:#}", e);
                eprintln!("  Hint: This is most likely a bug in EdgeDB \
                    or command-line tools. Please consider opening an \
                    issue ticket at \
                    https://github.com/edgedb/edgedb-cli/issues/new\
                    ?template=bug_report.md");
                exit(13);
            }
            eprintln!("edgedb error: {:#}", e);
            exit(1);
        }
    }
}

fn _main() -> anyhow::Result<()> {
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

    let opt = Options::from_args_and_env()?;

    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn")
    );
    log_levels::init(&mut builder, &opt);
    builder.init();

    version_check::check(opt.no_version_check);

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
