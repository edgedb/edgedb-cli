use std::fs;
use std::str::FromStr;

use anyhow::Context;
use fn_error_context::context;

use color_print::cformat;

use crate::commands::ExitCode;
use crate::cloud;
use crate::credentials;
use crate::hint::HintExt;
use crate::platform;
use crate::portable::control::{self, self_signed_arg, ensure_runstate_dir};
use crate::portable::exit_codes;
use crate::portable::install;
use crate::portable::local::{Paths, InstanceInfo};
use crate::portable::local::{write_json, allocate_port};
use crate::portable::options::{Create, InstanceName, Start};
use crate::portable::platform::optional_docker_check;
use crate::portable::repository::{Query, QueryOptions};
use crate::portable::reset_password::{password_hash, generate_password};
use crate::portable::{windows, linux, macos};
use crate::print::{self, echo, err_marker, Highlight};
use crate::process;
use crate::question;

use edgedb_tokio::credentials::Credentials;

fn ask_name(
    cloud_client: &mut cloud::client::CloudClient
) -> anyhow::Result<InstanceName> {
    let instances = credentials::all_instance_names()?;
    loop {
        let name = question::String::new(
            "Specify a name for the new instance"
        ).ask()?;
        let inst_name = match InstanceName::from_str(&name) {
            Ok(name) => name,
            Err(e) => {
                print::error(e);
                continue;
            }
        };
        let exists = match &inst_name {
            InstanceName::Local(name) => instances.contains(name),
            InstanceName::Cloud { org_slug, name} => {
                if !cloud_client.is_logged_in {
                    if let Err(e) = cloud::ops::prompt_cloud_login(cloud_client) {
                        print::error(e);
                        continue;
                    }
                }
                cloud::ops::find_cloud_instance_by_name(
                    name, org_slug, cloud_client
                )?.is_some()
            }
        };
        if exists {
            echo!(err_marker(), "Instance", name.emphasize(), "already exists.");
        } else {
            return Ok(inst_name);
        }
    }
}


pub fn create(cmd: &Create, opts: &crate::options::Options) -> anyhow::Result<()> {
    if optional_docker_check()? {
        print::error(
            "`edgedb instance create` is not supported in Docker containers.",
        );
        return Err(ExitCode::new(exit_codes::DOCKER_CONTAINER))?;
    }
    if cmd.start_conf.is_some() {
        print::warn("The option `--start-conf` is deprecated. \
                     Use `edgedb instance start/stop` to control \
                     the instance.");
    }

    let mut client = cloud::client::CloudClient::new(&opts.cloud_options)?;
    let inst_name = if let Some(name) = &cmd.name {
        name.to_owned()
    } else if cmd.non_interactive {
        echo!(err_marker(), "Instance name is required \
                             in non-interactive mode");
        return Err(ExitCode::new(2).into());
    } else {
        ask_name(&mut client)?
    };

    let name = match inst_name.clone() {
        InstanceName::Local(name) => name,
        InstanceName::Cloud { org_slug, name } => {
            create_cloud(cmd, opts, &org_slug, &name, &client)?;
            return Ok(())
        }
    };

    if let Some(cp) = &cmd.cloud_params {
        if cp.region.is_some() {
            return Err(opts.error(
                clap::error::ErrorKind::ArgumentConflict,
                cformat!("The <bold>--region</bold> option is only applicable to cloud instances."),
            ))?;
        }

        if cp.compute_size.is_some() {
            return Err(opts.error(
                clap::error::ErrorKind::ArgumentConflict,
                cformat!("The <bold>--compute-size</bold> option is only applicable to cloud instances."),
            ))?;
        }

        if cp.storage_size.is_some() {
            return Err(opts.error(
                clap::error::ErrorKind::ArgumentConflict,
                cformat!("The <bold>--storage-size</bold> option is only applicable to cloud instances."),
            ))?;
        }
    }

    let paths = Paths::get(&name)?;
    paths.check_exists()
        .with_context(|| format!("instance {:?} detected", name))
        .with_hint(|| format!("Use `edgedb destroy {}` \
                              to remove rest of unused instance",
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
        let (query, _) = Query::from_options(
            QueryOptions {
                nightly: cmd.nightly,
                testing: false,
                channel: cmd.channel,
                version: cmd.version.as_ref(),
                stable: false,
            },
            || anyhow::Ok(Query::stable()),
        )?;
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
                instance: Some(inst_name),
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

fn create_cloud(
    cmd: &Create,
    opts: &crate::options::Options,
    org_slug: &str,
    name: &str,
    client: &cloud::client::CloudClient,
) -> anyhow::Result<()> {
    let inst_name = InstanceName::Cloud {
        org_slug: org_slug.to_string(),
        name: name.to_string(),
    };

    client.ensure_authenticated()?;

    let cp = cmd.cloud_params.as_ref();

    let region = match &cp.and_then(|p| p.region.clone()) {
        None => cloud::ops::get_current_region(&client)?.name,
        Some(region) => region.to_string(),
    };

    let org = cloud::ops::get_org(org_slug, client)?;

    let (query, _) = Query::from_options(
        QueryOptions {
            nightly: cmd.nightly,
            testing: false,
            channel: cmd.channel,
            version: cmd.version.as_ref(),
            stable: false,
        },
        || anyhow::Ok(Query::stable()),
    )?;

    let server_ver = cloud::versions::get_version(&query, client)?;

    let compute_size = cp.and_then(|p| p.compute_size);
    let storage_size = cp.and_then(|p| p.storage_size);

    let tier = if let Some(tier) = cp.and_then(|p| p.tier) {
        tier
    } else if compute_size.is_some() || storage_size.is_some() || org.preferred_payment_method.is_some() {
        cloud::ops::CloudTier::Pro
    } else {
        cloud::ops::CloudTier::Free
    };

    if tier == cloud::ops::CloudTier::Free {
        if compute_size.is_some() {
            return Err(opts.error(
                clap::error::ErrorKind::ArgumentConflict,
                cformat!("The <bold>--compute-size</bold> option can \
                only be specified for Pro instances."),
            ))?;
        }
        if storage_size.is_some() {
            return Err(opts.error(
                clap::error::ErrorKind::ArgumentConflict,
                cformat!("The <bold>--storage-size</bold> option can \
                only be specified for Pro instances."),
            ))?;
        }
    }

    let compute_size_v: String;
    let storage_size_v: String;
    let mut req_resources: Vec<cloud::ops::CloudInstanceResourceRequest> = vec![];

    if org.preferred_payment_method.is_some() {
        compute_size_v = compute_size.unwrap_or(1).to_string();
        storage_size_v = storage_size.unwrap_or(20).to_string();
    } else {
        compute_size_v = compute_size.and_then(|v| Some(v.to_string()))
            .unwrap_or(String::from("1/4"));
        storage_size_v = storage_size.and_then(|v| Some(v.to_string()))
            .unwrap_or(String::from("1"));
    }

    if let Some(compute_size) = compute_size {
        req_resources.push(
            cloud::ops::CloudInstanceResourceRequest{
                name: "compute".to_string(),
                value: compute_size,
            },
        )
    }

    if let Some(storage_size) = storage_size {
        req_resources.push(
            cloud::ops::CloudInstanceResourceRequest{
                name: "storage".to_string(),
                value: storage_size,
            },
        )
    }

    let resources_display = format!(
        "\nCompute Size: {} compute unit{}\
        \nStorage Size: {} gigabyte{}",
        compute_size_v,
        if compute_size_v == "1" {""} else {"s"},
        storage_size_v,
        if storage_size_v == "1" {""} else {"s"},
    );

    if !cmd.non_interactive && !question::Confirm::new(format!(
        "This will create a new EdgeDB cloud instance with the following parameters:\
        \n\
        \nTier: {tier:?}\
        \nRegion: {region}\
        \nServer Version: {server_ver}\
        {resources_display}\
        \n\nIs this acceptable?",
    )).ask()? {
        return Ok(());
    }

    let request = cloud::ops::CloudInstanceCreate {
        name: name.to_string(),
        org: org_slug.to_string(),
        version: server_ver.to_string(),
        region: Some(region),
        requested_resources: Some(req_resources),
        tier: Some(tier),
    };
    cloud::ops::create_cloud_instance(&client, &request)?;
    echo!(
        "EdgeDB Cloud instance",
        inst_name,
        "is up and running."
    );
    echo!("To connect to the instance run:");
    echo!("  edgedb -I", inst_name);
    return Ok(())
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
    credentials::write(&paths.credentials, &creds)?;

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
