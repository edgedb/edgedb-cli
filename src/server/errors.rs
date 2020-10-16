#[derive(Debug, thiserror::Error)]
#[error("instance not found")]
pub struct InstanceNotFound(#[source] pub anyhow::Error);

#[derive(Debug, thiserror::Error)]
#[error("cannot create service")]
pub struct CannotCreateService(#[source] pub anyhow::Error);

#[derive(Debug, thiserror::Error)]
#[error("cannot start service")]
pub struct CannotStartService(#[source] pub anyhow::Error);
