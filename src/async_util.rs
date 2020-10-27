use std::io;
use std::time::Duration;

use async_std::future::Future;


pub async fn timeout<F, T>(dur: Duration, f: F) -> anyhow::Result<T>
    where F: Future<Output = anyhow::Result<T>>,
{
    use async_std::future::timeout;

    timeout(dur, f).await
    .unwrap_or_else(|_| Err(io::Error::from(io::ErrorKind::TimedOut).into()))
}

