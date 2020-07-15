use crate::options::{Options, Command};
use crate::commands::parser::Common;
use crate::server::options::Command as Server;


pub fn init(builder: &mut env_logger::Builder, opt: &Options) {
    if opt.debug_print_frames {
        builder.filter_module("edgedb::incoming::frame",
                              log::LevelFilter::Debug);
    }
    match &opt.subcommand {
        Some(Command::Common(Common::Restore(r))) if r.verbose => {
            builder.filter_module("edgedb::restore", log::LevelFilter::Info);
        }
        Some(Command::Server(s)) => match &s.subcommand {
            Server::Upgrade(u) if u.verbose => {
                builder.filter_module(
                    "edgedb::server::upgrade", log::LevelFilter::Info);
            }
            _ => {}
        },
        _ => {}
    }
}
