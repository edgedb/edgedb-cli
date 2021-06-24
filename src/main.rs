// We don't need to hunt of unused imports on windows, as they are harmless
#![cfg_attr(windows, allow(unused_imports))]
#![type_length_limit="8388608"]

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
mod directory_check;
mod error_display;
mod format;
mod highlight;
mod hint;
mod interactive;
mod log_levels;
mod migrations;
mod non_interactive;
mod options;
mod outputs;
mod platform;
mod print;
mod process;
mod project;
mod prompt;
mod question;
mod repl;
mod self_install;
mod self_migrate;
mod self_upgrade;
mod server;
mod statement;
mod table;
mod variables;
mod version_check;

#[macro_use] mod markdown;

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
            eprintln!("edgedb error: {:#}", err);
            for item in err.chain() {
                if let Some(e) = item.downcast_ref::<hint::HintedError>() {
                    eprintln!("  Hint: {}", e.hint
                        .lines()
                        .collect::<Vec<_>>()
                        .join("\n        "));
                } else if item.is::<bug::Bug>() {
                    eprintln!("  Hint: This is most likely a bug in EdgeDB \
                        or command-line tools. Please consider opening an \
                        issue ticket at \
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

    log::debug!(target: "edgedb::cli", "Options: {:#?}", opt);

    version_check::check(opt.no_version_check);

    if opt.subcommand.is_some() {
        commands::cli::main(opt)
    } else {
        directory_check::check_and_warn();
        if opt.interactive {
            interactive::main(opt)
        } else {
            task::block_on(non_interactive::main(opt))
        }
    }
}
