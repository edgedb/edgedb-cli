use std::path::Path;
use std::fs;

use crate::commands::ExitCode;
use crate::platform::tmp_file_path;
use crate::project::config::{self, SrcConfig};
use crate::project::options::Upgrade;
use crate::project::{self, project_dir, stash_path};
use crate::server;
use crate::server::control;
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

    if !to_version.is_nightly() && inst.get_version()? == &to_version {
        println!("Major version matches. Running a minor version upgrade.");
        inst.method().upgrade(
            &upgrade::ToDo::MinorUpgrade, &server::options::Upgrade {
                local_minor: true,
                to_latest: false,
                to_version: None,
                to_nightly: false,
                name: Some(instance_name.into()),
                verbose: options.verbose,
                force: options.force,
            })?;
    } else {
        inst.method().upgrade(
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
        let new_inst = inst.method().get_instance(&instance_name)?;
        println!("Instance upgraded to {}",
            new_inst.get_current_version()?.unwrap());
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
        modify_toml(&config_path, &version)?;
        println!("Config updated successfully. \
            Run `edgedb project init` to initialize an instance.")
    } else {
        let os = detect::current_os()?;
        let methods = os.all_methods()?;
        let text = fs::read_to_string(stash_dir.join("instance-name"))?;
        let instance_name = text.trim();
        let inst = control::get_instance(&methods, &instance_name)?;
        inst.method().upgrade(
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
        modify_toml(&config_path, &version)?;
        println!("Instance upgraded to {}",
            new_inst.get_current_version()?.unwrap());
        println!("Remember to commit it to the version control.");
    };
    Ok(())
}

#[context("cannot modify `{}`", config.display())]
fn modify_toml(config: &Path, ver: &MajorVersion) -> anyhow::Result<()> {
    println!("Setting `server-version = {:?}` in `edgedb.toml`", ver.title());
    let input = fs::read_to_string(&config)?;
    let output = toml_set_version(&input, ver.as_str())?;
    let tmp = tmp_file_path(config);
    fs::remove_file(&tmp).ok();
    fs::write(&tmp, output)?;
    fs::rename(&tmp, config)?;
    Ok(())
}

fn toml_set_version(data: &str, version: &str) -> anyhow::Result<String> {
    use std::fmt::Write;

    let mut toml = toml::de::Deserializer::new(&data);
    let parsed: SrcConfig = serde_path_to_error::deserialize(&mut toml)?;
    if let Some(ver_position) = &parsed.edgedb.server_version {
        let mut out = String::with_capacity(data.len() + 5);
        write!(&mut out, "{}{:?}{}",
            &data[..ver_position.start()],
            version,
            &data[ver_position.end()..],
        ).unwrap();
        return Ok(out);
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

    #[test_case(TOML_BETA1, "1-beta2" => TOML_BETA2)]
    #[test_case(TOML_BETA2, "1-beta2" => TOML_BETA2)]
    #[test_case(TOML_NIGHTLY, "1-beta2" => TOML_BETA2)]
    #[test_case(TOML_BETA1, "1-beta1" => TOML_BETA1)]
    #[test_case(TOML_BETA2, "1-beta1" => TOML_BETA1)]
    #[test_case(TOML_NIGHTLY, "1-beta1" => TOML_BETA1)]
    #[test_case(TOML_BETA1, "nightly" => TOML_NIGHTLY)]
    #[test_case(TOML_BETA2, "nightly" => TOML_NIGHTLY)]
    #[test_case(TOML_NIGHTLY, "nightly" => TOML_NIGHTLY)]

    #[test_case(TOML2_BETA1, "1-beta2" => TOML2_BETA2)]
    #[test_case(TOML2_BETA2, "1-beta2" => TOML2_BETA2)]
    #[test_case(TOML2_NIGHTLY, "1-beta2" => TOML2_BETA2)]
    #[test_case(TOML2_BETA1, "1-beta1" => TOML2_BETA1)]
    #[test_case(TOML2_BETA2, "1-beta1" => TOML2_BETA1)]
    #[test_case(TOML2_NIGHTLY, "1-beta1" => TOML2_BETA1)]
    #[test_case(TOML2_BETA1, "nightly" => TOML2_NIGHTLY)]
    #[test_case(TOML2_BETA2, "nightly" => TOML2_NIGHTLY)]
    #[test_case(TOML2_NIGHTLY, "nightly" => TOML2_NIGHTLY)]

    #[test_case(TOMLI_BETA1, "1-beta2" => TOMLI_BETA2)]
    #[test_case(TOMLI_BETA2, "1-beta2" => TOMLI_BETA2)]
    #[test_case(TOMLI_NIGHTLY, "1-beta2" => TOMLI_BETA2)]
    #[test_case(TOMLI_BETA1, "1-beta1" => TOMLI_BETA1)]
    #[test_case(TOMLI_BETA2, "1-beta1" => TOMLI_BETA1)]
    #[test_case(TOMLI_NIGHTLY, "1-beta1" => TOMLI_BETA1)]
    #[test_case(TOMLI_BETA1, "nightly" => TOMLI_NIGHTLY)]
    #[test_case(TOMLI_BETA2, "nightly" => TOMLI_NIGHTLY)]
    #[test_case(TOMLI_NIGHTLY, "nightly" => TOMLI_NIGHTLY)]
    fn set(src: &str, ver: &str) -> String {
        toml_set_version(src, ver).unwrap()
    }

}
