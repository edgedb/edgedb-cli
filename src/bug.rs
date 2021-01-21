#[derive(Debug, thiserror::Error)]
#[error("bug detected: {}", _0)]
pub struct Bug(String);

/// Make an instance of the error
///
/// We return anyhow errors to also have a backtrace
pub fn error(err: impl Into<String>) -> anyhow::Error {
    Bug(err.into()).into()
}
