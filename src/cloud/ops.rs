use std::time::{Duration, Instant};

use async_std::task;
use edgedb_client::credentials::Credentials;
use edgedb_client::Builder;

use crate::cloud::client::CloudClient;
use crate::commands::ExitCode;
use crate::credentials;
use crate::options::CloudOptions;
use crate::portable::local::is_valid_instance_name;
use crate::portable::status::{RemoteStatus, RemoteType, ConnectionStatus};
use crate::print::{self, echo, err_marker, Highlight};
use crate::question;

const INSTANCE_CREATION_WAIT_TIME: Duration = Duration::from_secs(5 * 60);
const INSTANCE_CREATION_POLLING_INTERVAL : Duration = Duration::from_secs(1);

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

impl RemoteStatus {
    fn from_cloud_instance(cloud_instance: &CloudInstance) -> anyhow::Result<Self> {
        Ok(Self {
            name: format!("{}/{}", cloud_instance.org_slug, cloud_instance.name),
            type_: RemoteType::Cloud {
                instance_id: cloud_instance.id.clone(),
            },
            credentials: cloud_instance.as_credentials()?,
            version: None,
            connection: ConnectionStatus::Cloud {
                status: cloud_instance.status.clone(),
            },
        })
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
    inst: &str,
    org: &str,
    client: &CloudClient,
) -> anyhow::Result<Option<CloudInstance>> {
    let instance: CloudInstance = client.get(format!("orgs/{}/instances/{}", org, inst)).await?;
    Ok(Some(instance))
}

async fn wait_instance_create(
    mut instance: CloudInstance,
    client: &CloudClient,
    quiet: bool,
) -> anyhow::Result<CloudInstance> {
    if !quiet && instance.status == "creating" {
        print::echo!("Waiting for EdgeDB Cloud instance creation...");
    }
    let url = format!("orgs/{}/instances/{}", instance.org_slug, instance.name);
    let deadline = Instant::now() + INSTANCE_CREATION_WAIT_TIME;
    while Instant::now() < deadline {
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
            task::sleep(INSTANCE_CREATION_POLLING_INTERVAL).await;
        }
        instance = client.get(&url).await?;
    }
    if instance.dsn != "" {
        Ok(instance)
    } else {
        anyhow::bail!("Timed out.")
    }
}

pub async fn create_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceCreate,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances", instance.org);
    let instance: CloudInstance = client
        .post(url, serde_json::to_value(instance)?)
        .await?;
    wait_instance_create(instance, client, false).await?;
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

pub async fn list(client: CloudClient) -> anyhow::Result<Vec<RemoteStatus>> {
    client.ensure_authenticated(false)?;
    let cloud_instances: Vec<CloudInstance> = client.get("instances/").await?;
    let mut rv = Vec::new();
    for cloud_instance in cloud_instances {
        match RemoteStatus::from_cloud_instance(&cloud_instance) {
            Ok(status) => rv.push(status),
            Err(e) => {
                log::warn!(
                    "Cannot check cloud instance {}/{}: {:#}",
                    cloud_instance.org_slug,
                    cloud_instance.name,
                    e);
            }
        }
    }
    Ok(rv)
}
