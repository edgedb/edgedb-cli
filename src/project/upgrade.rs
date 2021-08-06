use std::path::Path;
use std::fs;

use anyhow::Context;

use crate::commands::ExitCode;
use crate::platform::tmp_file_path;
use crate::project::config::{self, SrcConfig};
use crate::project::options::Upgrade;
use crate::project::{self, project_dir, stash_path};
use crate::question;
use crate::server;
use crate::server::control;
use crate::server::destroy;
use crate::server::detect::{self, VersionQuery};
use crate::server::distribution::{MajorVersion};
use crate::server::upgrade;

use fn_error_context::context;


pub fn upgrade(options: &Upgrade) -> anyhow::Result<()> {
    if options.to_version.is_some() ||
        options.to_nightly || options.to_latest
    {
        update_toml(&options)
    } else {
        upgrade_instance(&options)
    }
}

pub fn upgrade_instance(options: &Upgrade) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let config_path = root.join("edgedb.toml");
    let config = config::read(&config_path)?;
    let to_version = if let Some(ver) = config.edgedb.server_version {
        ver
    } else {
        anyhow::bail!("No version specified in `edgedb.toml`.");
    };

    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        anyhow::bail!("No instance initialized.");
    }
    let text = fs::read_to_string(stash_dir.join("instance-name"))?;
    let instance_name = text.trim();

    let os = detect::current_os()?;
    let methods = os.all_methods()?;
    let inst = control::get_instance(&methods, &instance_name)?;
    let mut should_upgrade = inst.get_version()? != &to_version;

    if should_upgrade {
        if !question::Confirm::new(format!(
            "Do you want to upgrade to {} per edgedb.toml?", to_version.title()
        )).ask()? {
            should_upgrade = false;
        }
    } else {
        let version_query = to_version.to_query();
        let method = inst.method();
        let new_version = method.get_version(&version_query)
            .context("Unable to determine version")?;
        if let Some(old_version) = upgrade::get_installed(
            &version_query, method
        )? {
            if &old_version < new_version.version() {
                if to_version.is_nightly() {
                    if question::Confirm::new(format!(
                        "A new nightly version is available: {}. \
                        Do you want to upgrade?", new_version.version()
                    )).ask()? {
                        should_upgrade = true;
                    }
                } else {
                    println!("A new minor version is available: {}",
                             new_version.version());
                    println!("  Run `edgedb instance upgrade --local-minor` \
                             to update.");
                }
            } else {
                println!("Instance is up to date.")
            }
        }
    }
    if should_upgrade {
        let upgraded = inst.method().upgrade(
            &upgrade::ToDo::InstanceUpgrade(
                instance_name.to_string(),
                Some(to_version.to_query()),
            ), &server::options::Upgrade {
                local_minor: false,
                to_latest: false,
                to_version: options.to_version.clone(),
                to_nightly: to_version.is_nightly(),
                name: Some(instance_name.into()),
                verbose: options.verbose,
                force: options.force,
            })?;
        if upgraded {
            let new_inst = inst.method().get_instance(&instance_name)?;
            println!("Instance upgraded to {}",
                     new_inst.get_current_version()?.unwrap());
            if !inst.get_version()?.is_nightly() {
                print_other_project_warning(
                    instance_name, &root, &to_version
                )?;
            }
        } else {
            println!("Instance is up to date.")
        }
    }
    Ok(())
}

pub fn update_toml(options: &Upgrade) -> anyhow::Result<()> {
    let root = project_dir(options.project_dir.as_ref().map(|x| x.as_path()))?;
    let config_path = root.join("edgedb.toml");
    let config = config::read(&config_path)?;

    let to_version = if let Some(ver) = &options.to_version {
        Some(MajorVersion::Stable(ver.clone()))
    } else if options.to_nightly {
        Some(MajorVersion::Nightly)
    } else {
        match config.edgedb.server_version {
            Some(ver) if ver.is_nightly() => Some(MajorVersion::Nightly),
            _ => None,
        }
    };

    let stash_dir = stash_path(&root)?;
    if !stash_dir.exists() {
        log::warn!("No associated instance found.");
        let version = if let Some(ver) = to_version {
            ver
        } else {
            let os = detect::current_os()?;
            let meth = os.any_method()?;
            let distr = meth.get_version(&VersionQuery::Stable(None))?;
            distr.major_version().clone()
        };
        if modify_toml(&config_path, &version)? {
            println!("Config updated successfully. \
            Run `edgedb project init` to initialize an instance.")
        } else {
            println!("Config is up to date. \
            Run `edgedb project init` to initialize an instance.")
        }
    } else {
        let os = detect::current_os()?;
        let methods = os.all_methods()?;
        let text = fs::read_to_string(stash_dir.join("instance-name"))?;
        let instance_name = text.trim();
        let inst = control::get_instance(&methods, &instance_name)?;
        let upgraded = inst.method().upgrade(
            &upgrade::ToDo::InstanceUpgrade(
                instance_name.to_string(),
                to_version.map(|x| x.to_query()),
            ), &server::options::Upgrade {
                local_minor: false,
                to_latest: false,
                to_version: options.to_version.clone(),
                to_nightly: options.to_nightly,
                name: Some(instance_name.into()),
                verbose: options.verbose,
                force: options.force,
            })?;
        // re-read instance to invalidate cache in the object
        let new_inst = inst.method().get_instance(&instance_name)?;
        let version = new_inst.get_version()?;
        if modify_toml(&config_path, &version)? {
            println!("Remember to commit it to version control.");
        }
        if upgraded {
            println!("Instance upgraded to {}",
                     new_inst.get_current_version()?.unwrap());
            print_other_project_warning(instance_name, &root, version)?;
        } else {
            println!("Instance is up to date.")
        }
    };
    Ok(())
}

#[context("cannot modify `{}`", config.display())]
fn modify_toml(config: &Path, ver: &MajorVersion) -> anyhow::Result<bool> {
    let input = fs::read_to_string(&config)?;
    if let Some(output) = toml_set_version(&input, ver.as_str())? {
        println!("Setting `server-version = {:?}` in `edgedb.toml`", ver.title());
        let tmp = tmp_file_path(config);
        fs::remove_file(&tmp).ok();
        fs::write(&tmp, output)?;
        fs::rename(&tmp, config)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn toml_set_version(data: &str, version: &str) -> anyhow::Result<Option<String>> {
    use std::fmt::Write;

    let mut toml = toml::de::Deserializer::new(&data);
    let parsed: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;
    if let Some(ver_position) = &parsed.edgedb.server_version {
        if ver_position.get_ref().as_str() == version {
            return Ok(None);
        }
        let mut out = String::with_capacity(data.len() + 5);
        write!(&mut out, "{}{:?}{}",
            &data[..ver_position.start()],
            version,
            &data[ver_position.end()..],
        ).unwrap();
        return Ok(Some(out));
    }
    eprintln!("No server-version found in `edgedb.toml`.");
    eprintln!("Please ensure that `edgedb.toml` contains:");
    println!("  {}",
        project::init::format_config(version)
        .lines()
        .collect::<Vec<_>>()
        .join("\n  "));
    return Err(ExitCode::new(2).into());
}

fn print_other_project_warning(
    name: &str, project_path: &Path, to_version: &MajorVersion
) -> anyhow::Result<()> {
    let mut project_dirs = Vec::new();
    for pd in destroy::find_project_dirs(name)? {
        let real_pd = match destroy::read_project_real_path(&pd) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("edgedb error: {}", e);
                continue;
            }
        };
        if real_pd != project_path {
            project_dirs.push(real_pd);
        }
    }
    if !project_dirs.is_empty() {
        eprintln!("Warning: the instance {} is still used by the following \
                  projects:", name);
        for pd in &project_dirs {
            eprintln!("  {}", pd.display());
        }
        eprintln!("Run the following commands to update them:");
        let version = match to_version {
            MajorVersion::Nightly => "--to-nightly".into(),
            MajorVersion::Stable(version) => {
                format!("--to-version {}", version)
            }
        };
        let current_project = project::project_dir_opt(None)?;
        for pd in &project_dirs {
            upgrade::print_project_upgrade_command(
                &version, &current_project, pd
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use test_case::test_case;
    use super::toml_set_version;

    const TOML_BETA1: &str = "\
        [edgedb]\n\
        server-version = \"1-beta1\"\n\
    ";
    const TOML_BETA2: &str = "\
        [edgedb]\n\
        server-version = \"1-beta2\"\n\
    ";
    const TOML_NIGHTLY: &str = "\
        [edgedb]\n\
        server-version = \"nightly\"\n\
    ";

    const TOML2_BETA1: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1-beta1\" #and here\n\
        other-setting = true\n\
    ";
    const TOML2_BETA2: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"1-beta2\" #and here\n\
        other-setting = true\n\
    ";
    const TOML2_NIGHTLY: &str = "\
        [edgedb]\n\
        # some comment\n\
        server-version = \"nightly\" #and here\n\
        other-setting = true\n\
    ";

    const TOMLI_BETA1: &str = "\
        edgedb = {server-version = \"1-beta1\"}\n\
    ";
    const TOMLI_BETA2: &str = "\
        edgedb = {server-version = \"1-beta2\"}\n\
    ";
    const TOMLI_NIGHTLY: &str = "\
        edgedb = {server-version = \"nightly\"}\n\
    ";

    #[test_case(TOML_BETA1, "1-beta2" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA2, "1-beta2" => None)]
    #[test_case(TOML_NIGHTLY, "1-beta2" => Some(TOML_BETA2.into()))]
    #[test_case(TOML_BETA1, "1-beta1" => None)]
    #[test_case(TOML_BETA2, "1-beta1" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_NIGHTLY, "1-beta1" => Some(TOML_BETA1.into()))]
    #[test_case(TOML_BETA1, "nightly" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_BETA2, "nightly" => Some(TOML_NIGHTLY.into()))]
    #[test_case(TOML_NIGHTLY, "nightly" => None)]

    #[test_case(TOML2_BETA1, "1-beta2" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA2, "1-beta2" => None)]
    #[test_case(TOML2_NIGHTLY, "1-beta2" => Some(TOML2_BETA2.into()))]
    #[test_case(TOML2_BETA1, "1-beta1" => None)]
    #[test_case(TOML2_BETA2, "1-beta1" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_NIGHTLY, "1-beta1" => Some(TOML2_BETA1.into()))]
    #[test_case(TOML2_BETA1, "nightly" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_BETA2, "nightly" => Some(TOML2_NIGHTLY.into()))]
    #[test_case(TOML2_NIGHTLY, "nightly" => None)]

    #[test_case(TOMLI_BETA1, "1-beta2" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA2, "1-beta2" => None)]
    #[test_case(TOMLI_NIGHTLY, "1-beta2" => Some(TOMLI_BETA2.into()))]
    #[test_case(TOMLI_BETA1, "1-beta1" => None)]
    #[test_case(TOMLI_BETA2, "1-beta1" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_NIGHTLY, "1-beta1" => Some(TOMLI_BETA1.into()))]
    #[test_case(TOMLI_BETA1, "nightly" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_BETA2, "nightly" => Some(TOMLI_NIGHTLY.into()))]
    #[test_case(TOMLI_NIGHTLY, "nightly" => None)]
    fn set(src: &str, ver: &str) -> Option<String> {
        toml_set_version(src, ver).unwrap()
    }

}
