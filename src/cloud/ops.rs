use std::time::{Duration, Instant};

use anyhow::Context;
use edgedb_tokio::Builder;
use edgedb_tokio::credentials::Credentials;
use indicatif::ProgressBar;
use tokio::time::{sleep, timeout};

use crate::cloud::client::{CloudClient, ErrorResponse};
use crate::collect::Collector;
use crate::options::CloudOptions;
use crate::portable::status::{RemoteStatus, RemoteType, try_connect};
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
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tls_ca: Option<String>,
}

impl CloudInstance {
    pub async fn as_credentials(&self, secret_key: &str) -> anyhow::Result<Credentials> {
        let config = Builder::new()
            .secret_key(secret_key)
            .instance(&format!("{}/{}", self.org_slug, self.name))?
            .build_env().await?;
        let mut creds = config.as_credentials()?;
        // TODO(tailhook) can this be emitted from as_credentials()?
        creds.tls_ca = self.tls_ca.clone();
        Ok(creds)
    }
}

impl RemoteStatus {
    async fn from_cloud_instance(
        cloud_instance: &CloudInstance,
        secret_key: &str,
    ) -> anyhow::Result<Self> {
        let credentials = cloud_instance.as_credentials(secret_key).await?;
        let (version, connection) = try_connect(&credentials).await;
        Ok(Self {
            name: format!("{}/{}", cloud_instance.org_slug, cloud_instance.name),
            type_: RemoteType::Cloud {
                instance_id: cloud_instance.id.clone(),
            },
            credentials,
            version,
            connection: Some(connection),
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
    pub version: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub default_database: Option<String>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub default_user: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct CloudInstanceUpgrade {
    pub name: String,
    pub org: String,
    pub version: String,
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


#[tokio::main]
pub async fn find_cloud_instance_by_name(
    inst: &str,
    org: &str,
    client: &CloudClient,
) -> anyhow::Result<Option<CloudInstance>> {
    client
        .get(format!("orgs/{}/instances/{}", org, inst))
        .await
        .map(Some)
        .or_else(|e| match e.downcast_ref::<ErrorResponse>() {
            Some(ErrorResponse { code: reqwest::StatusCode::NOT_FOUND, .. }) => Ok(None),
            _ => Err(e),
        })
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
                sleep(POLLING_INTERVAL).await;
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

#[tokio::main]
pub async fn create_cloud_instance(
    client: &CloudClient,
    request: &CloudInstanceCreate,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances", request.org);
    let operation: CloudOperation = client
        .post(url, request)
        .await
        .or_else(|e| match e.downcast_ref::<ErrorResponse>() {
            Some(ErrorResponse { code: reqwest::StatusCode::NOT_FOUND, .. }) => {
                anyhow::bail!("The organization \"{}\" doesn't exist.", request.org);
            }
            _ => Err(e),
        })?;
    wait_instance_create(operation, &request.org, &request.name, client).await?;
    Ok(())
}

#[tokio::main]
pub async fn upgrade_cloud_instance(
    client: &CloudClient,
    request: &CloudInstanceUpgrade,
) -> anyhow::Result<()> {
    let url = format!("orgs/{}/instances/{}", request.org, request.name);
    let operation: CloudOperation = client
        .put(url, request)
        .await?;
    wait_instance_upgrade(operation, &request.org, &request.name, client).await?;
    Ok(())
}

pub fn prompt_cloud_login(client: &mut CloudClient) -> anyhow::Result<()> {
    let mut q = question::Confirm::new(
        "You're not authenticated to the EdgeDB Cloud yet, login now?",
    );
    if q.default(true).ask()? {
        crate::cloud::auth::do_login(&client)?;
        client.reinit()?;
        client.ensure_authenticated()?;
        Ok(())
    } else {
        anyhow::bail!("Aborted.");
    }
}

#[tokio::main]
pub async fn try_to_destroy(name: &str, org: &str, options: &CloudOptions) -> anyhow::Result<()> {
    log::info!("Destroying EdgeDB Cloud instance: {}/{}", name, org);
    let client = CloudClient::new(options)?;
    client.ensure_authenticated()?;
    let _: CloudOperation = client.delete(
        format!("orgs/{}/instances/{}", org, name)
    ).await?;
    Ok(())
}

async fn get_instances(client: CloudClient) -> anyhow::Result<Vec<CloudInstance>> {
    timeout(Duration::from_secs(30), client.get("instances/"))
        .await
        .or_else(|_| anyhow::bail!("timed out with Cloud API"))?
        .context("failed with Cloud API")
}

pub async fn list(
    client: CloudClient,
    errors: &Collector<anyhow::Error>,
) -> anyhow::Result<Vec<RemoteStatus>> {
    client.ensure_authenticated()?;
    let secret_key = client.secret_key.clone().unwrap();
    let cloud_instances = get_instances(client).await?;
    let mut rv = Vec::new();
    for cloud_instance in cloud_instances {
        match RemoteStatus::from_cloud_instance(&cloud_instance, &secret_key).await {
            Ok(status) => rv.push(status),
            Err(e) => {
                errors.add(
                    e.context(format!(
                        "probing {}/{}",
                        cloud_instance.org_slug, cloud_instance.name
                    ))
                );
            }
        }
    }
    Ok(rv)
}
