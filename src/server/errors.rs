#[derive(Debug, thiserror::Error)]
#[error("instance not found")]
pub struct InstanceNotFound(#[source] pub anyhow::Error);
