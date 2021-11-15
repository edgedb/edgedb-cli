use std::fs;
use std::io;
use std::path::Path;

use anyhow::Context;
use async_std::task;
use fn_error_context::context;

use crate::commands::ExitCode;
use crate::hint::HintExt;
use crate::platform;
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{Paths, InstanceInfo};
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{Query};
use crate::portable::{windows, linux, macos};
use crate::print::{self, eecho, Highlight};
use crate::process;
use crate::server::create::allocate_port;
use crate::server::options::{Create, StartConf};
use crate::server::reset_password::{password_hash, generate_password};
use crate::server::reset_password::{write_credentials};

use edgedb_client::credentials::Credentials;


pub fn create(options: &Create) -> anyhow::Result<()> {
    if options.method.is_some() {
        return crate::server::create::create(options);
    }
    if optional_docker_check()? {
        print::error(
            "`edgedb instance create` in a Docker container is not supported.",
        );
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }

    let paths = Paths::get(&options.name)?;
    paths.check_exists()
        .with_context(|| format!("instance {:?} detected", options.name))
        .with_hint(|| format!("Use `edgedb destroy {}` \
                              to remove remains of unused instance",
                              options.name))?;

    let query = Query::from_options(options.nightly, &options.version)?;
    let inst = install::version(&query).context("error installing EdgeDB")?;
    let port = allocate_port(&options.name)?;
    let info = InstanceInfo {
        name: options.name.clone(),
        installation: inst,
        port,
        start_conf: options.start_conf,
    };
    bootstrap(&paths, &info,
              &options.default_database, &options.default_user)?;

    match (create_service(&options.name, &info), options.start_conf) {
        (Ok(()), StartConf::Manual) => {
            eecho!("Instance", options.name.emphasize(), "is ready.");
            eprintln!("You can start it manually via: \n  \
                edgedb instance start [--foreground] {}",
                options.name);
        }
        (Ok(()), StartConf::Auto) => {
            eecho!("Instance", options.name.emphasize(), "is up and running.");
        }
        (Err(e), _) => {
            eprintln!("Bootstrapping complete, \
                but there was an error creating the service: {:#}", e);
            eprintln!("You can start it manually via: \n  \
                edgedb instance start --foreground {}",
                options.name);
            return Err(ExitCode::new(2))?;
        }
    }
    Ok(())
}

#[context("cannot write metadata {:?}", path)]
fn write_meta(path: &Path, data: &InstanceInfo) -> anyhow::Result<()> {
    let file = io::BufWriter::new(fs::File::create(path)?);
    serde_json::to_writer_pretty(file, data)?;
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
    let server_path = info.installation.server_path()?;

    let tmp_data = platform::tmp_file_path(&paths.data_dir);
    if tmp_data.exists() {
        fs::remove_dir_all(&tmp_data)
            .with_context(|| format!("removing {:?}", &tmp_data))?;
    }
    fs::create_dir_all(&tmp_data)
            .with_context(|| format!("creating {:?}", &tmp_data))?;

    let password = generate_password();
    let script = bootstrap_script(database, user, &password);

    eecho!("Initializing EdgeDB instance...");
    process::Native::new("bootstrap", "edgedb", server_path)
        .arg("--bootstrap-only")
        .env_default("EDGEDB_SERVER_LOG_LEVEL", "warn")
        .arg("--data-dir").arg(&tmp_data)
        .arg("--generate-self-signed-cert")
        .arg("--bootstrap-command").arg(script)
        .run()?;

    let cert_path = tmp_data.join("edbtlscert.pem");
    let cert = fs::read_to_string(&cert_path)
        .with_context(|| format!("cannot read certificate: {:?}", cert_path))?;

    write_meta(&tmp_data.join("instance_info.json"), &info)?;
    fs::rename(&tmp_data, &paths.data_dir)
        .with_context(|| format!("renaming {:?} -> {:?}",
                                 tmp_data, paths.data_dir))?;

    let mut creds = Credentials::default();
    creds.port = info.port;
    creds.user = user.into();
    creds.database = Some(database.into());
    creds.password = Some(password.into());
    creds.tls_cert_data = Some(cert);
    task::block_on(write_credentials(&paths.credentials, &creds))?;

    Ok(())
}

pub fn create_service(name: &str, meta: &InstanceInfo) -> anyhow::Result<()>
{
    if cfg!(target_os="macos") {
        macos::create_service(&name, &meta)
    } else if cfg!(target_os="linux") {
        linux::create_service(&name, &meta)
    } else if cfg!(windows) {
        windows::create_service(&name, &meta)
    } else {
        anyhow::bail!("creating a service is not supported on the platform");
    }
}
