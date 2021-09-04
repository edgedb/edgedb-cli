use std::{collections::BTreeSet};

use anyhow::Context;

use crate::print;
use crate::project::config;
use crate::project::options::Link;
use crate::project::{project_dir, stash_path, write_stash_dir};
use crate::question;
use crate::server::control::get_instance;
use crate::server::detect::{self, VersionQuery};
use crate::server::is_valid_name;
use crate::server::methods::Methods;

pub fn link(options: &Link) -> anyhow::Result<()> {
    let project_dir = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let stash_dir = stash_path(&project_dir)?;

    if stash_dir.exists() {
        anyhow::bail!("Project is already linked");
    }

    let config_path = project_dir.join("edgedb.toml");
    let config = config::read(&config_path)?;
    let ver_query = match config.edgedb.server_version {
        None => VersionQuery::Stable(None),
        Some(ver) => ver.to_query(),
    };

    let os = detect::current_os()?;
    let avail_methods = os.get_available_methods()?;
    let methods = avail_methods.instantiate_all(&*os, true)?;

    let name = ask_instance_name(&methods, options)?;

    let instance = get_instance(&methods, &name)?;
    let version = instance.get_version()?;
    if !ver_query.matches(version) {
        print::warn(format!(
            "WARNING: existing instance has version {}, \
                but {} is required by `edgedb.toml`",
            version.title(),
            ver_query
        ));
    }

    write_stash_dir(&stash_dir, &project_dir, &name)?;

    print::success("Project linked");
    if let Some(dir) = &options.project_dir {
        println!(
            "To connect to {}, navigate to {} and run `edgedb`",
            name,
            dir.display()
        );
    } else {
        println!("To connect to {}, run `edgedb`", name);
    }

    Ok(())
}


fn ask_instance_name(methods: &Methods, options: &Link) -> anyhow::Result<String> {
    let instances = methods
        .values()
        .map(|m| m.all_instances())
        .collect::<Result<Vec<_>, _>>()
        .context("failed to enumerate existing instances")?
        .into_iter()
        .flatten()
        .map(|inst| inst.name().to_string())
        .collect::<BTreeSet<_>>();

    if let Some(name) = &options.name {
        if instances.contains(name) {
            return Ok(name.clone());
        }

        print::error(format!("Instance {:?} doesn't exist", name));
    }

    if options.non_interactive {
        anyhow::bail!("Existing instance name should be specified")
    }

    let mut q =
        question::String::new("Specify the name of EdgeDB instance to link with this project");
    loop {
        let target_name = q.ask()?;
        if !is_valid_name(&target_name) {
            print::error(
                "Instance name must be a valid identifier, \
                         (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
            );
            continue;
        }
        if instances.contains(&target_name) {
            return Ok(target_name);
        } else {
            print::error(format!("Instance {:?} doesn't exist", target_name));
        }
    }
}
