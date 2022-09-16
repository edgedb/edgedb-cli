use std::fs;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;

use crate::commands::ExitCode;
use crate::cloud;
use crate::credentials;
use crate::hint::HintExt;
use crate::platform;
use crate::portable::control::{self, self_signed_arg, ensure_runstate_dir};
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{Paths, InstanceInfo};
use crate::portable::local::{write_json, allocate_port, is_valid_instance_name};
use crate::portable::options::{Create, Start};
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{Query};
use crate::portable::reset_password::{password_hash, generate_password};
use crate::portable::{windows, linux, macos};
use crate::print::{self, echo, err_marker, Highlight};
use crate::process;
use crate::question;

use edgedb_client::credentials::Credentials;


fn ask_name() -> anyhow::Result<String> {
    let instances = credentials::all_instance_names()?;
    loop {
        let name = question::String::new(
            "Specify a name for the new instance"
        ).ask()?;
        if !is_valid_instance_name(&name) {
            echo!(err_marker(),
                "Instance name must be a valid identifier, \
                 (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)");
            continue;
        }
        if instances.contains(&name) {
            echo!(err_marker(),
                "Instance", name.emphasize(), "already exists.");
            continue;
        }
        return Ok(name);
    }
}


pub fn create(cmd: &Create, opts: &crate::options::Options) -> anyhow::Result<()> {
    if optional_docker_check()? {
        print::error(
            "`edgedb instance create` in a Docker container is not supported.",
        );
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    if cmd.start_conf.is_some() {
        print::warn("The option `--start-conf` is deprecated. \
                     Use `edgedb instance start/stop` to control \
                     the instance.");
    }

    let name = if let Some(name) = &cmd.name {
        name.to_owned()
    } else if cmd.non_interactive {
        echo!(err_marker(), "Instance name is required \
                             in non-interactive mode");
        return Err(ExitCode::new(2).into());
    } else {
        ask_name()?
    };

    if name.contains("/") {
        return task::block_on(cloud::ops::create(cmd, opts));
    };

    let paths = Paths::get(&name)?;
    paths.check_exists()
        .with_context(|| format!("instance {:?} detected", name))
        .with_hint(|| format!("Use `edgedb destroy {}` \
                              to remove remains of unused instance",
                              name))?;

    let port = cmd.port.map(Ok)
        .unwrap_or_else(|| allocate_port(&name))?;

    let info = if cfg!(windows) {
        windows::create_instance(cmd, &name, port, &paths)?;
        InstanceInfo {
            name: name.clone(),
            installation: None,
            port,
        }
    } else {
        let query = Query::from_options(cmd.nightly, &cmd.version)?;
        let inst = install::version(&query).context("error installing EdgeDB")?;
        let info = InstanceInfo {
            name: name.clone(),
            installation: Some(inst),
            port,
        };
        bootstrap(&paths, &info,
                  &cmd.default_database, &cmd.default_user)?;
        info
    };

    if windows::is_wrapped() {
        // no service and no messages
        return Ok(())
    }

    match create_service(&info) {
        Ok(()) => {},
        Err(e) => {
            log::warn!("Error running EdgeDB as a service: {e:#}");
            print::warn("EdgeDB will not start on next login. \
                         Trying to start database in the background...");
            control::start(&Start {
                name: None,
                instance: Some(info.name.clone()),
                foreground: false,
                auto_restart: false,
                managed_by: None,
            })?;
        }
    }

    echo!("Instance", name.emphasize(), "is up and running.");
    echo!("To connect to the instance run:");
    echo!("  edgedb -I", name);
    Ok(())
}

pub fn bootstrap_script(database: &str, user: &str, password: &str) -> String {
    use std::fmt::Write;
    use edgeql_parser::helpers::{quote_string, quote_name};

    let mut output = String::with_capacity(1024);
    if database != "edgedb" {
        write!(&mut output,
            "CREATE DATABASE {};",
            quote_name(&database),
        ).unwrap();
    }
    if user == "edgedb" {
        write!(&mut output, r###"
            ALTER ROLE {name} {{
                SET password_hash := {password_hash};
            }};
            "###,
            name=quote_name(&user),
            password_hash=quote_string(&password_hash(password)),
        ).unwrap();
    } else {
        write!(&mut output, r###"
            CREATE SUPERUSER ROLE {name} {{
                SET password_hash := {password_hash};
            }}"###,
            name=quote_name(&user),
            password_hash=quote_string(&password_hash(password)),
        ).unwrap();
    }
    return output;
}

#[context("cannot bootstrap EdgeDB instance")]
pub fn bootstrap(paths: &Paths, info: &InstanceInfo,
                 database: &str, user: &str)
    -> anyhow::Result<()>
{
    let server_path = info.server_path()?;

    let tmp_data = platform::tmp_file_path(&paths.data_dir);
    if tmp_data.exists() {
        fs::remove_dir_all(&tmp_data)
            .with_context(|| format!("removing {:?}", &tmp_data))?;
    }
    fs::create_dir_all(&tmp_data)
            .with_context(|| format!("creating {:?}", &tmp_data))?;

    let password = generate_password();
    let script = bootstrap_script(database, user, &password);

    echo!("Initializing EdgeDB instance...");
    let mut cmd = process::Native::new("bootstrap", "edgedb", server_path);
    cmd.arg("--bootstrap-only");
    cmd.env_default("EDGEDB_SERVER_LOG_LEVEL", "warn");
    cmd.arg("--data-dir").arg(&tmp_data);
    cmd.arg("--runstate-dir").arg(&ensure_runstate_dir(&info.name)?);
    self_signed_arg(&mut cmd, info.get_version()?);
    cmd.arg("--bootstrap-command").arg(script);
    cmd.run()?;

    let cert_path = tmp_data.join("edbtlscert.pem");
    let cert = fs::read_to_string(&cert_path)
        .with_context(|| format!("cannot read certificate: {:?}", cert_path))?;

    write_json(&tmp_data.join("instance_info.json"), "metadata", &info)?;
    fs::rename(&tmp_data, &paths.data_dir)
        .with_context(|| format!("renaming {:?} -> {:?}",
                                 tmp_data, paths.data_dir))?;

    let mut creds = Credentials::default();
    creds.port = info.port;
    creds.user = user.into();
    creds.database = Some(database.into());
    creds.password = Some(password.into());
    creds.tls_ca = Some(cert);
    task::block_on(credentials::write(&paths.credentials, &creds))?;

    Ok(())
}

pub fn create_service(meta: &InstanceInfo) -> anyhow::Result<()>
{
    if cfg!(target_os="macos") {
        macos::create_service(&meta)
    } else if cfg!(target_os="linux") {
        if windows::is_wrapped() {
            // No service. Managed by windows.
            // Note: in `create` we avoid even calling this function because
            // we need to print message from windows. But on upgrade, revert,
            // etc. completion message is printed from the linux, so this
            // function is called.
            Ok(())
        } else {
            linux::create_service(&meta)
        }
    } else if cfg!(windows) {
        windows::create_service(&meta)
    } else {
        anyhow::bail!("creating a service is not supported on the platform");
    }
}
