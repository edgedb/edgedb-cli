use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use async_std::task;
use colorful::Colorful;
use edgedb_client::credentials::Credentials;
use edgedb_client::Builder;

use crate::cloud::auth;
use crate::cloud::client::CloudClient;
use crate::credentials;
use crate::options::CloudOptions;
use crate::print::{self, Highlight};
use crate::question;
use crate::table::{self, Cell, Row, Table};

const INSTANCE_CREATE_WAIT_TIME: i32 = 5 * 60;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CloudInstance {
    id: String,
    name: String,
    dsn: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_ca: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct InstanceStatus {
    cloud_instance: CloudInstance,
    credentials: Option<Credentials>,
    instance_name: Option<String>,
}

impl InstanceStatus {
    fn from_cloud_instance(cloud_instance: CloudInstance) -> Self {
        Self {
            cloud_instance,
            credentials: None,
            instance_name: None,
        }
    }

    fn print_extended(&self) {
        println!("{}:", self.cloud_instance.name);

        println!("  Status: {}", self.cloud_instance.status);
        println!("  ID: {}", self.cloud_instance.id);
        if let Some(name) = &self.instance_name {
            println!("  Local Instance: {}", name);
        }
        if let Some(creds) = &self.credentials {
            if let Some(dsn) = &creds.cloud_original_dsn {
                println!("  DSN: {}", dsn);
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct Org {
    pub id: String,
    pub name: String,
}

#[derive(Debug, serde::Serialize)]
pub struct CloudInstanceCreate {
    pub name: String,
    pub org: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub version: Option<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub default_database: Option<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub default_user: Option<String>,
}

pub async fn find_cloud_instance_by_name(
    name: &str,
    client: &CloudClient,
) -> anyhow::Result<Option<CloudInstance>> {
    let instances: Vec<CloudInstance> = client.get("instances/").await?;
    if let Some(instance) = instances
        .into_iter()
        .find(|instance| instance.name.eq(&name))
    {
        Ok(Some(instance))
    } else {
        Ok(None)
    }
}

async fn wait_instance_create(
    mut instance: CloudInstance,
    client: &CloudClient,
    quiet: bool,
) -> anyhow::Result<CloudInstance> {
    for i in 0..INSTANCE_CREATE_WAIT_TIME {
        if instance.dsn != "" {
            return Ok(instance);
        }
        if instance.status != "available" && instance.status != "creating" {
            anyhow::bail!(
                "Failed to create EdgeDB Cloud instance: {}",
                instance.status
            );
        }
        if instance.status == "creating" {
            if i == 0 && !quiet {
                print::echo!("Waiting for EdgeDB Cloud instance creation...");
            }
            task::sleep(Duration::from_secs(1)).await;
        }
        instance = client.get(format!("instances/{}", instance.id)).await?;
    }
    if instance.dsn != "" {
        Ok(instance)
    } else {
        anyhow::bail!("Timed out.")
    }
}

async fn write_credentials(cred_path: &PathBuf, instance: CloudInstance) -> anyhow::Result<()> {
    let mut creds = Builder::uninitialized()
        .read_dsn(&instance.dsn)
        .await?
        .as_credentials()?;
    creds.tls_ca = instance.tls_ca;
    creds.cloud_instance_id = Some(instance.id);
    creds.cloud_original_dsn = Some(instance.dsn);
    credentials::write(&cred_path, &creds).await
}

pub async fn create_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceCreate,
) -> anyhow::Result<()> {
    let cred_path = credentials::path(&instance.name)?;
    if cred_path.exists() {
        anyhow::bail!("File {} exists; abort.", cred_path.display());
    }
    let instance: CloudInstance = client
        .post("instances/", serde_json::to_value(instance)?)
        .await?;
    let instance = wait_instance_create(instance, client, false).await?;
    write_credentials(&cred_path, instance).await?;
    Ok(())
}

pub async fn create(
    cmd: &crate::portable::options::Create,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    if !client.is_logged_in {
        anyhow::bail!("Run `edgedb cloud login` first.");
    };
    // let version = Query::from_options(cmd.nightly, &cmd.version)?;
    let orgs: Vec<Org> = client.get("orgs/").await?;
    let org_id = if let Some(name) = &cmd.cloud_org {
        if let Some(org) = orgs.iter().find(|org| org.name.eq(name)) {
            org.id.clone()
        } else {
            anyhow::bail!("Organization {} not found", name);
        }
    } else {
        // TODO: use default organization
        orgs[0].id.clone()
    };
    let instance = CloudInstanceCreate {
        name: cmd.name.clone(),
        org: org_id
        // version: Some(format!("{}", version.display())),
        // default_database: Some(cmd.default_database.clone()),
        // default_user: Some(cmd.default_user.clone()),
    };
    create_cloud_instance(&client, &instance).await?;
    print::echo!(
        "EdgeDB Cloud instance",
        cmd.name.emphasize(),
        "is up and running."
    );
    print::echo!("To connect to the instance run:");
    print::echo!("  edgedb -I", cmd.name);
    Ok(())
}

pub async fn link(
    cmd: &crate::portable::options::Link,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let mut client = CloudClient::new(&opts.cloud_options)?;
    if cmd.non_interactive {
        if let Some(name) = &cmd.name {
            if !crate::portable::local::is_valid_name(name) {
                print::error(
                    "Instance name must be a valid identifier, \
                             (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                );
            }
            let cred_path = credentials::path(name)?;
            if cred_path.exists() && !cmd.overwrite {
                anyhow::bail!("File {} exists; abort.", cred_path.display());
            }
        } else {
            anyhow::bail!("Name is mandatory if --non-interactive is set.");
        }
    }
    if !client.is_logged_in {
        if cmd.non_interactive {
            anyhow::bail!("Run `edgedb cloud login` first.");
        } else {
            let q = question::Confirm::new(
                "You're not authenticated to the EdgeDB Cloud yet, login now?",
            );
            if q.ask()? {
                auth::do_login(&client).await?;
                client = CloudClient::new(&opts.cloud_options)?;
                if !client.is_logged_in {
                    anyhow::bail!("Couldn't fetch access token.");
                }
            } else {
                print::error("Aborted.");
                return Ok(());
            }
        }
    };
    let cloud_name = if let Some(name) = &cmd.name {
        name.clone()
    } else {
        if cmd.non_interactive {
            unreachable!("Already checked previously");
        } else {
            loop {
                let name = question::String::new(
                    "Input the name of the EdgeDB Cloud instance to connect to",
                )
                .ask()?;
                if !crate::portable::local::is_valid_name(&name) {
                    print::error(
                        "Instance name must be a valid identifier, \
                                 (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                    );
                    continue;
                }
                break name;
            }
        }
    };
    let instance = if let Some(instance) = find_cloud_instance_by_name(&cloud_name, &client).await?
    {
        wait_instance_create(instance, &client, cmd.quiet).await?
    } else {
        anyhow::bail!("No such Cloud instance named {}", cloud_name);
    };

    let (cred_path, instance_name) = if let Some(name) = &cmd.name {
        let cred_path = credentials::path(&name)?;
        if cred_path.exists() && cmd.overwrite && !cmd.quiet {
            print::warn(format!("Overwriting {}", cred_path.display()));
        }
        (cred_path, name.clone())
    } else {
        assert!(!cmd.non_interactive, "Already checked previously");
        let same_name_exists = credentials::path(&cloud_name)?.exists() && !cmd.overwrite;
        loop {
            let name = if same_name_exists {
                question::String::new("Specify a local name to refer to the EdgeDB Cloud instance")
                    .ask()?
            } else {
                question::String::new("Use the same name locally?")
                    .default(&cloud_name)
                    .ask()?
            };
            if !crate::portable::local::is_valid_name(&name) {
                print::error(
                    "Instance name must be a valid identifier, \
                         (regex: ^[a-zA-Z_][a-zA-Z_0-9]*$)",
                );
                continue;
            }
            let cred_path = credentials::path(&name)?;
            if cred_path.exists() {
                if cmd.overwrite {
                    if !cmd.quiet {
                        print::warn(format!("Overwriting {}", cred_path.display()));
                    }
                } else {
                    let mut q = question::Confirm::new_dangerous(format!(
                        "{} exists! Overwrite?",
                        cred_path.display()
                    ));
                    q.default(false);
                    if !q.ask()? {
                        continue;
                    }
                }
            }
            break (cred_path, name);
        }
    };

    write_credentials(&cred_path, instance).await?;
    if !cmd.quiet {
        let mut msg = "Successfully linked to EdgeDB Cloud instance.".to_string();
        if print::use_color() {
            msg = format!("{}", msg.bold().light_green());
        }
        eprintln!(
            "{} To connect run:\
            \n  edgedb -I {}",
            msg,
            instance_name.escape_default(),
        );
    }
    Ok(())
}

async fn destroy(instance_id: &str, options: &CloudOptions) -> anyhow::Result<()> {
    log::info!("Destroying EdgeDB Cloud instance: {}", instance_id);
    let client = CloudClient::new(options)?;
    if !client.is_logged_in {
        anyhow::bail!("Cloud authentication required.");
    };
    let _: CloudInstance = client.delete(format!("instances/{}", instance_id)).await?;
    Ok(())
}

pub fn try_to_destroy(
    cred_path: &PathBuf,
    options: &crate::options::Options,
) -> anyhow::Result<()> {
    let file = io::BufReader::new(fs::File::open(cred_path)?);
    let credentials: Credentials = serde_json::from_reader(file)?;
    if let Some(instance_id) = credentials.cloud_instance_id {
        task::block_on(destroy(&instance_id, &options.cloud_options))?;
    }
    Ok(())
}

pub async fn list(
    cmd: &crate::portable::options::List,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    if !client.is_logged_in {
        anyhow::bail!("Run `edgedb cloud login` first.");
    };
    let cloud_instances: Vec<CloudInstance> = client.get("instances/").await?;
    let mut instances = cloud_instances
        .into_iter()
        .map(|inst| (inst.id.clone(), InstanceStatus::from_cloud_instance(inst)))
        .collect::<HashMap<String, InstanceStatus>>();
    for name in credentials::all_instance_names()? {
        let file = io::BufReader::new(fs::File::open(credentials::path(&name)?)?);
        let creds: Credentials = serde_json::from_reader(file)?;
        if let Some(id) = &creds.cloud_instance_id {
            if let Some(instance) = instances.get_mut(id) {
                (*instance).instance_name = Some(name);
                (*instance).credentials = Some(creds);
            }
        }
    }
    if instances.is_empty() {
        if cmd.json {
            println!("[]");
        } else if !cmd.quiet {
            print::warn("No instances found");
        }
        return Ok(());
    }
    if cmd.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&instances.into_values().collect::<Vec<_>>())?
        );
    } else if cmd.debug {
        for instance in instances.values() {
            println!("{:#?}", instance);
        }
    } else if cmd.extended {
        for instance in instances.values() {
            instance.print_extended();
        }
    } else {
        let mut table = Table::new();
        table.set_format(*table::FORMAT);
        table.set_titles(Row::new(
            ["Kind", "Name", "Cloud Name", "Status"]
                .iter()
                .map(|x| table::header_cell(x))
                .collect(),
        ));
        for instance in instances.values() {
            table.add_row(Row::new(vec![
                Cell::new("cloud"),
                Cell::new(instance.instance_name.as_deref().unwrap_or("-")),
                Cell::new(&instance.cloud_instance.name),
                Cell::new(&instance.cloud_instance.status),
            ]));
        }
        table.printstd();
    }
    Ok(())
}
