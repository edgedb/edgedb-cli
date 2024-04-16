use std::future::Future;
use std::io;
use std::time::Duration;

pub async fn timeout<F, T, E>(dur: Duration, f: F) -> anyhow::Result<T>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error>,
{
    tokio::time::timeout(dur, f)
        .await
        .map(|r| r.map_err(Into::into))
        .unwrap_or_else(|_| Err(io::Error::from(io::ErrorKind::TimedOut).into()))
}

#[macro_export]
macro_rules! async_try {
    ($block:expr, finally $finally:expr) => {{
        let result = $block.await;
        let finally = $finally.await;
        match (finally, result) {
            (Ok(()), Ok(res)) => Ok(res),
            (Err(e), Ok(_)) => Err(e.into()),
            (Ok(()), Err(e)) => Err(e),
            (Err(fin_e), Err(e)) => {
                log::info!("Cannot finalize operation: {:#}", fin_e);
                Err(e)
            }
        }
    }};
    ($block:expr, except $except:expr, else $else:expr) => {{
        let block = $block.await;
        match block {
            Ok(result) => $else.await.map_err(Into::into).and(Ok(result)),
            Err(e) => {
                $except
                    .await
                    .map_err(|e| {
                        log::info!("Cannot cancel operation: {:#}", e);
                    })
                    .ok();
                Err(e)
            }
        }
    }};
}
