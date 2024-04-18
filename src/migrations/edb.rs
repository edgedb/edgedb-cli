use edgedb_errors::{ClientConnectionEosError, Error, ErrorKind};
use edgedb_protocol::queryable::Queryable;

use crate::connect::Connection;

pub async fn execute(cli: &mut Connection, text: impl AsRef<str>) -> Result<(), Error> {
    if !cli.is_consistent() {
        return Err(ClientConnectionEosError::with_message(
            "connection closed by server",
        ));
    }
    log_execute(cli, text).await
}

pub async fn execute_if_connected(
    cli: &mut Connection,
    text: impl AsRef<str>,
) -> Result<(), Error> {
    if !cli.is_consistent() {
        return Ok(());
    }
    log_execute(cli, text).await
}

async fn log_execute(cli: &mut Connection, text: impl AsRef<str>) -> Result<(), Error> {
    let text = text.as_ref();
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.execute(text, &()).await?;
    Ok(())
}

pub async fn query_row<R>(cli: &mut Connection, text: &str) -> Result<R, Error>
where
    R: Queryable,
{
    log::debug!(target: "edgedb::migrations::query", "Executing `{}`", text);
    cli.query_required_single(text, &()).await
}
