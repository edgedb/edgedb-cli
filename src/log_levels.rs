use crate::options::{Options, Command};
use crate::commands::parser::Common;
use crate::server::options::Command as Server;
use crate::project::options::Command as Project;


pub fn init(builder: &mut env_logger::Builder, opt: &Options) {
    if opt.debug_print_frames {
        builder.filter_module("edgedb::incoming::frame",
                              log::LevelFilter::Debug);
    }
    match &opt.subcommand {
        Some(Command::SelfUpgrade(s)) if s.verbose => {
            builder.filter_module("edgedb::self_upgrade",
                log::LevelFilter::Info);
        }
        Some(Command::Common(Common::Restore(r))) if r.verbose => {
            builder.filter_module("edgedb::restore", log::LevelFilter::Info);
        }
        Some(Command::Common(Common::CreateMigration(c)))
        if c.debug_print_queries
        => {
            builder.filter_module("edgedb::migrations::query",
                log::LevelFilter::Debug);
        }
        Some(Command::Server(s)) => match &s.subcommand {
            Server::Uninstall(u) if u.verbose => {
                builder.filter_module(
                    "edgedb::server::uninstall", log::LevelFilter::Info);
            }
            Server::Upgrade(u) if u.verbose => {
                builder.filter_module(
                    "edgedb::server::upgrade", log::LevelFilter::Info);
            }
            Server::Destroy(d) if d.verbose => {
                builder.filter_module(
                    "edgedb::server::destroy", log::LevelFilter::Info);
            }
            _ => {}
        },
        Some(Command::Project(p)) => match &p.subcommand {
            Project::Upgrade(u) if u.verbose => {
                builder.filter_module(
                    "edgedb::server::upgrade", log::LevelFilter::Info);
                builder.filter_module(
                    "edgedb::project::upgrade", log::LevelFilter::Info);
            }
            _ => {}
        }
        _ => {}
    }
}
