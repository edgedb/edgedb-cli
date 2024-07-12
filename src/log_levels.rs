use crate::cli::options::Command as Cli;
use crate::commands::parser::Common;
use crate::migrations::options::MigrationCmd;
use crate::options::{Command, Options};
use crate::portable::options::Command as Server;
use crate::portable::options::InstanceCommand as Instance;
use crate::portable::project::Command as Project;
use std::io::Write;

pub fn init(builder: &mut env_logger::Builder, opt: &Options) {
    if opt.debug_print_frames {
        builder.filter_module("edgedb::incoming::frame", log::LevelFilter::Debug);
    }
    match &opt.subcommand {
        Some(Command::Cli(c)) => match &c.subcommand {
            Cli::Upgrade(s) if s.verbose => {
                builder.filter_module("edgedb::self_upgrade", log::LevelFilter::Info);
            }
            Cli::Migrate(s) if s.verbose => {
                builder.filter_module("edgedb::self_migrate", log::LevelFilter::Info);
            }
            _ => {}
        },
        Some(Command::Common(Common::Restore(r))) if r.verbose => {
            builder.filter_module("edgedb::restore", log::LevelFilter::Info);
            builder.format(|buf, record| writeln!(buf, "{}", record.args()));
        }
        Some(Command::Watch(w)) if w.verbose => {
            builder.filter_module("edgedb::migrations::dev_mode::ddl", log::LevelFilter::Info);
        }
        Some(Command::Common(Common::Migration(c))) => match &c.subcommand {
            MigrationCmd::Create(c) if c.debug_print_queries => {
                builder.filter_module("edgedb::migrations::query", log::LevelFilter::Debug);
            }
            _ => {}
        },
        Some(Command::Server(s)) => match &s.subcommand {
            Server::Uninstall(u) if u.verbose => {
                builder.filter_module("edgedb::portable::uninstall", log::LevelFilter::Info);
            }
            _ => {}
        },
        Some(Command::Instance(i)) => match &i.subcommand {
            Instance::Destroy(d) if d.verbose => {
                builder.filter_module("edgedb::portable::destroy", log::LevelFilter::Info);
            }
            Instance::Upgrade(u) if u.verbose => {
                builder.filter_module("edgedb::portable::upgrade", log::LevelFilter::Info);
            }
            _ => {}
        },
        Some(Command::Project(p)) => match &p.subcommand {
            Project::Upgrade(u) if u.verbose => {
                builder.filter_module("edgedb::project::upgrade", log::LevelFilter::Info);
                builder.filter_module("edgedb::portable::upgrade", log::LevelFilter::Info);
                builder.filter_module("edgedb::portable::project", log::LevelFilter::Info);
            }
            _ => {}
        },
        _ => {}
    }
}
