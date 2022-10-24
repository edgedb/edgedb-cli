use std::collections::HashMap;
use std::fs;
use std::io;
use std::time::{Duration, Instant};

use async_std::task;
use edgedb_client::credentials::Credentials;
use edgedb_client::Builder;
use indicatif::ProgressBar;

use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::credentials;
use crate::options::CloudOptions;
use crate::portable::local::is_valid_instance_name;
use crate::print::{self, echo, err_marker, Highlight};
use crate::question;
use crate::table::{self, Cell, Row, Table};

const OPERATION_WAIT_TIME: Duration = Duration::from_secs(5 * 60);
const POLLING_INTERVAL: Duration = Duration::from_secs(1);
const SPINNER_TICK: Duration = Duration::from_millis(100);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CloudInstance {
    id: String,
    name: String,
    org_slug: String,
    dsn: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_ca: Option<String>,
}

impl CloudInstance {
    pub fn as_credentials(&self) -> anyhow::Result<Credentials> {
        let mut creds = task::block_on(Builder::uninitialized().read_dsn(&self.dsn))?.as_credentials()?;
        creds.tls_ca = self.tls_ca.clone();
        creds.cloud_instance_id = Some(self.id.clone());
        creds.cloud_original_dsn = Some(self.dsn.clone());
        Ok(creds)
    }
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

#[derive(Debug, serde::Serialize)]
pub struct CloudInstanceUpgrade {
    pub name: String,
    pub org: String,
}

pub async fn find_cloud_instance_by_name(
    inst: &str,
    org: &str,
    client: &CloudClient,
) -> anyhow::Result<Option<CloudInstance>> {
    let instance: CloudInstance = client.get(format!("orgs/{}/instances/{}", org, inst)).await?;
    Ok(Some(instance))
}

async fn wait_instance_available_after_operation(
    mut instance: CloudInstance,
    client: &CloudClient,
    operation: &str,
) -> anyhow::Result<CloudInstance> {
    let spinner = ProgressBar::new_spinner()
        .with_message(format!("Waiting for the result of EdgeDB Cloud instance {}...", operation));
    spinner.enable_steady_tick(SPINNER_TICK);

    let url = format!("orgs/{}/instances/{}", instance.org_slug, instance.name);
    let deadline = Instant::now() + OPERATION_WAIT_TIME;
    while Instant::now() < deadline {
        if instance.status != "available" && instance.status != operation {
            anyhow::bail!(
                "Failed to wait for EdgeDB Cloud instance to become available after {} an instance: {}",
                operation,
                instance.status
            );
        }
        if instance.status == operation {
            task::sleep(POLLING_INTERVAL).await;
            instance = client.get(&url).await?;
        } else {
            break;
        }
    }
    if instance.dsn != "" && instance.status == "available" {
        Ok(instance)
    } else {
        anyhow::bail!("Timed out.")
    }
}

async fn wait_instance_create(
    instance: CloudInstance,
    client: &CloudClient,
) -> anyhow::Result<CloudInstance> {
    wait_instance_available_after_operation(instance, client, "creating").await
}

async fn wait_instance_upgrade(
    instance: CloudInstance,
    client: &CloudClient,
) -> anyhow::Result<CloudInstance> {
    wait_instance_available_after_operation(instance, client, "upgrading").await
}

pub async fn create_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceCreate,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances", instance.org);
    let instance: CloudInstance = client
        .post(url, serde_json::to_value(instance)?)
        .await?;
    wait_instance_create(instance, client).await?;
    Ok(())
}

pub async fn upgrade_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceUpgrade,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances/{}", instance.org, instance.name);
    let instance: CloudInstance = client
        .put(url, serde_json::to_value(instance)?)
        .await?;
    wait_instance_upgrade(instance, client).await?;
    Ok(())
}

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

pub fn split_cloud_instance_name(name: &str) -> anyhow::Result<(String, String)> {
    let mut splitter = name.splitn(2, '/');
    match splitter.next() {
        None => unreachable!(),
        Some("") => anyhow::bail!("empty instance name"),
        Some(org) => match splitter.next() {
            None => anyhow::bail!("cloud instance must be in the form ORG/INST"),
            Some("") => anyhow::bail!("invalid instance name: missing instance"),
            Some(inst) => Ok((String::from(org), String::from(inst))),
        },
    }
}

pub async fn create(
    cmd: &crate::portable::options::Create,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated(false)?;

    let name = if let Some(name) = &cmd.name {
        name.to_owned()
    } else if cmd.non_interactive {
        echo!(err_marker(), "Instance name is required \
                             in non-interactive mode");
        return Err(ExitCode::new(2).into());
    } else {
        ask_name()?
    };

    let (org, inst_name) = split_cloud_instance_name(&name)?;
    let instance = CloudInstanceCreate {
        name: inst_name.clone(),
        org,
        // version: Some(format!("{}", version.display())),
        // default_database: Some(cmd.default_database.clone()),
        // default_user: Some(cmd.default_user.clone()),
    };
    create_cloud_instance(&client, &instance).await?;
    print::echo!(
        "EdgeDB Cloud instance",
        name.emphasize(),
        "is up and running."
    );
    print::echo!("To connect to the instance run:");
    print::echo!("  edgedb -I", name);
    Ok(())
}

pub async fn upgrade(
    cmd: &crate::portable::options::Upgrade,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated(false)?;

    let name = if let Some(name) = &cmd.name {
        name.to_owned()
    } else {
        ask_name()?
    };

    let (org, inst_name) = split_cloud_instance_name(&name)?;
    let instance = CloudInstanceUpgrade {
        name: inst_name.clone(),
        org,
    };
    upgrade_cloud_instance(&client, &instance).await?;
    print::echo!(
        "EdgeDB Cloud instance",
        name.emphasize(),
        "is successfully upgraded.",
    );
    Ok(())
}

async fn destroy(name: &str, org: &str, options: &CloudOptions) -> anyhow::Result<()> {
    log::info!("Destroying EdgeDB Cloud instance: {}/{}", name, org);
    let client = CloudClient::new(options)?;
    client.ensure_authenticated(false)?;
    let _: CloudInstance = client.delete(format!("orgs/{}/instances/{}", org, name)).await?;
    Ok(())
}

pub fn try_to_destroy(
    name: &str,
    org: &str,
    options: &crate::options::Options,
) -> anyhow::Result<()> {
    task::block_on(destroy(name, org, &options.cloud_options))?;
    Ok(())
}

pub async fn list(
    cmd: &crate::portable::options::List,
    opts: &crate::options::Options,
) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated(false)?;
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
            ["Kind", "Name", "Status"]
                .iter()
                .map(|x| table::header_cell(x))
                .collect(),
        ));
        for instance in instances.values() {
            table.add_row(Row::new(vec![
                Cell::new("cloud"),
                Cell::new(&format!("{}/{}", instance.cloud_instance.org_slug, instance.cloud_instance.name)),
                Cell::new(&instance.cloud_instance.status),
            ]));
        }
        table.printstd();
    }
    Ok(())
}
