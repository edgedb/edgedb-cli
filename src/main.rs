// We don't need to hunt of unused imports on windows, as they are harmless
#![cfg_attr(windows, allow(unused_imports))]
#![type_length_limit="8388608"]

use std::default::Default;
use std::env;
use std::path::Path;
use std::process::exit;

use async_std::task;
use clap::{Parser};

use crate::options::Options;

mod async_util;
mod bug;
mod cli;
mod commands;
mod completion;
mod connect;
mod config;
mod credentials;
mod error_display;
mod format;
mod highlight;
mod hint;
mod interactive;
mod interrupt;
mod log_levels;
mod markdown;
mod migrations;
mod non_interactive;
mod options;
mod outputs;
mod platform;
mod portable;
mod cloud;
mod print;
mod process;
//mod project;
mod prompt;
mod question;
mod repl;
//mod server;
mod statement;
mod table;
mod tty_password;
mod variables;
mod version_check;

fn main() {
    match _main() {
        Ok(()) => {}
        Err(ref e) => {
            let mut err = e;
            let mut code = 1;
            if let Some(e) = err.downcast_ref::<commands::ExitCode>() {
                e.exit();
            }
            if let Some(arc) = err.downcast_ref::<hint::ArcError>() {
                // prevent duplicate error message
                err = arc.inner();
            }
            if let Some(e) = err.downcast_ref::<edgedb_client::errors::Error>() {
                print::edgedb_error(e, true);
            } else {
                print::error(err);
            }
            for item in err.chain() {
                if let Some(e) = item.downcast_ref::<hint::HintedError>() {
                    eprintln!("  Hint: {}", e.hint
                        .lines()
                        .collect::<Vec<_>>()
                        .join("\n        "));
                } else if item.is::<bug::Bug>() {
                    eprintln!("  Hint: This is most likely a bug in EdgeDB \
                        or command-line tools. Please consider opening an \
                        issue at \
                        https://github.com/edgedb/edgedb-cli/issues/new\
                        ?template=bug_report.md");
                    code = 13;
                } else if let Some(e) = e.downcast_ref::<commands::ExitCode>()
                {
                    code = e.code();
                }
            }
            exit(code);
        }
    }
}

fn is_cli_upgrade(cmd: &Option<options::Command>) -> bool {
    use options::Command::CliCommand as Cli;
    use cli::options::CliCommand;
    use cli::options::Command::Upgrade;
    matches!(cmd, Some(Cli(CliCommand { subcommand: Upgrade(..) })))
}

fn _main() -> anyhow::Result<()> {
    // If a crash happens we want the backtrace to be printed by default
    // to ease bug reporting and troubleshooting.
    // TODO: consider removing this once EdgeDB reaches 1.0 stable.
    env::set_var("RUST_BACKTRACE", "1");
    interrupt::init_signals();

    if let Some(arg0) = std::env::args_os().next() {
        if let Some(exe_name) = Path::new(&arg0).file_name() {
            if exe_name.to_string_lossy().contains("-init") {
                let opt = cli::install::CliInstall::parse();
                return cli::install::main(&opt);
            }
        }
    }

    let opt = Options::from_args_and_env()?;
    let cfg = config::get_config();

    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn")
    );
    log_levels::init(&mut builder, &opt);
    builder.init();

    let cfg = cfg.unwrap_or_else(|e| {
        log::warn!("Config error: {:#}", e);
        Default::default()
    });

    log::debug!(target: "edgedb::cli", "Options: {:#?}", opt);

    if !is_cli_upgrade(&opt.subcommand) {
        version_check::check(opt.no_cli_update_check)?;
    }

    if opt.subcommand.is_some() {
        commands::cli::main(opt)
    } else {
        cli::directory_check::check_and_warn();
        if opt.interactive {
            interactive::main(opt, cfg)
        } else {
            task::block_on(non_interactive::interpret_stdin(
                &opt,
                opt.output_format.unwrap_or(repl::OutputFormat::JsonPretty)
            ))
        }
    }
}
