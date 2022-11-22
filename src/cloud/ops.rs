use std::time::{Duration, Instant};

use async_std::future::timeout;
use async_std::task;
use edgedb_client::credentials::Credentials;
use edgedb_client::Builder;
use indicatif::ProgressBar;

use crate::cloud::client::CloudClient;
use crate::options::CloudOptions;
use crate::portable::status::{RemoteStatus, RemoteType};
use crate::question;

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
            connection: None,
            instance_status: Some(cloud_instance.status.clone()),
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

#[derive(Debug, serde::Serialize)]
pub struct CloudInstanceUpgrade {
    pub name: String,
    pub org: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum OperationStatus {
    InProgress,
    Failed,
    Completed
}

#[derive(Debug, serde::Deserialize)]
pub struct CloudOperation {
    pub id: String,
    pub status: OperationStatus,
    pub message: String,
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
    mut operation: CloudOperation,
    org: &str,
    name: &str,
    client: &CloudClient,
    operation_type: &str,
) -> anyhow::Result<CloudInstance> {
    let spinner = ProgressBar::new_spinner()
        .with_message(format!("Waiting for the result of EdgeDB Cloud instance {}...", operation_type));
    spinner.enable_steady_tick(SPINNER_TICK);

    let url = format!("operations/{}", operation.id);
    let deadline = Instant::now() + OPERATION_WAIT_TIME;
    while Instant::now() < deadline {
        match operation.status {
            OperationStatus::Failed => {
                anyhow::bail!(
                    "Failed to wait for EdgeDB Cloud instance to become available after {} an instance: {}",
                    operation_type,
                    operation.message,
                );
            },
            OperationStatus::InProgress => {
                task::sleep(POLLING_INTERVAL).await;
                operation = client.get(&url).await?;
            }
            OperationStatus::Completed => {
                break;
            }
        }
    }

    let url = format!("orgs/{}/instances/{}", org, name);
    let instance: CloudInstance = client.get(&url).await?;

    if instance.dsn != "" && instance.status == "available" {
        Ok(instance)
    } else {
        anyhow::bail!("Timed out.")
    }
}

async fn wait_instance_create(
    operation: CloudOperation,
    org: &str,
    name: &str,
    client: &CloudClient,
) -> anyhow::Result<CloudInstance> {
    wait_instance_available_after_operation(operation, org, name, client, "creating").await
}

async fn wait_instance_upgrade(
    operation: CloudOperation,
    org: &str,
    name: &str,
    client: &CloudClient,
) -> anyhow::Result<CloudInstance> {
    wait_instance_available_after_operation(operation, org, name, client, "upgrading").await
}

pub async fn create_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceCreate,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances", instance.org);
    let operation: CloudOperation = client
        .post(url, serde_json::to_value(instance)?)
        .await?;
    wait_instance_create(operation, &instance.org, &instance.name, client).await?;
    Ok(())
}

pub async fn upgrade_cloud_instance(
    client: &CloudClient,
    instance: &CloudInstanceUpgrade,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances/{}", instance.org, instance.name);
    let operation: CloudOperation = client
        .put(url, serde_json::to_value(instance)?)
        .await?;
    wait_instance_upgrade(operation, &instance.org, &instance.name, client).await?;
    Ok(())
}

pub async fn prompt_cloud_login(client: &mut CloudClient) -> anyhow::Result<()> {
    let mut q = question::Confirm::new(
        "You're not authenticated to the EdgeDB Cloud yet, login now?",
    );
    if q.default(true).ask()? {
        crate::cloud::auth::do_login(&client).await?;
        client.reinit()?;
        client.ensure_authenticated()?;
        Ok(())
    } else {
        anyhow::bail!("Aborted.");
    }
}

pub async fn upgrade(org: &str, name: &str, opts: &crate::options::Options) -> anyhow::Result<()> {
    let client = CloudClient::new(&opts.cloud_options)?;
    client.ensure_authenticated()?;

    let instance = CloudInstanceUpgrade {
        org: org.to_string(),
        name: name.to_string(),
    };
    upgrade_cloud_instance(&client, &instance).await?;
    Ok(())
}

async fn destroy(name: &str, org: &str, options: &CloudOptions) -> anyhow::Result<()> {
    log::info!("Destroying EdgeDB Cloud instance: {}/{}", name, org);
    let client = CloudClient::new(options)?;
    client.ensure_authenticated()?;
    let _: CloudOperation = client.delete(format!("orgs/{}/instances/{}", org, name)).await?;
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
    client.ensure_authenticated()?;
    let cloud_instances: Vec<CloudInstance> = timeout(
        Duration::from_secs(30), client.get("instances/")
    ).await??;
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
