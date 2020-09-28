use std::path::Path;

use anyhow::Context;
use async_std::fs;
use async_std::io;
use fn_error_context::context;
use serde::de::DeserializeOwned;


#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, thiserror::Error)]
#[error("HTTP failure: {} {}",
        self.0.status(), self.0.status().canonical_reason())]
pub struct HttpFailure(surf::Response);


trait HttpErrorExt<T> {
    fn url_context(self, url: &str) -> Result<T, anyhow::Error>;
}
trait HttpOkExt<T> {
    fn ensure200(self, url: &str) -> Result<T, anyhow::Error>;
}
trait HttpErrExt<T> {
    fn context(self, context: &'static str) -> Result<T, anyhow::Error>;
}

impl HttpOkExt<surf::Response> for Result<surf::Response, surf::Error> {
    fn ensure200(self, url: &str)
        -> Result<surf::Response, anyhow::Error>
    {
        match self {
            Ok(res) if res.status() != 200 => {
                Err(HttpFailure(res)).url_context(url)
            }
            Err(e) => Err(HttpError(e)).url_context(url),
            Ok(res) => Ok(res),
        }
    }
}

impl<T> HttpErrExt<T> for Result<T, surf::Error> {
    fn context(self, context: &'static str) -> Result<T, anyhow::Error> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(HttpError(e)).context(context),
        }
    }
}

impl<T, E> HttpErrorExt<T> for Result<T, E>
    where Result<T, E>: Context<T, E>
{
    fn url_context(self, url: &str) -> Result<T, anyhow::Error> {
        self.with_context(|| format!("fetching {:?}", url))
    }
}

#[context("failed to fetch URL: {}", url)]
pub async fn get_string(url: &str)
    -> Result<String, anyhow::Error>
{
    log::info!("Fetching {}", url);
    Ok(surf::get(url).await.ensure200(url)?
        .body_string().await.map_err(HttpError).url_context(url)?)
}

#[context("failed to fetch JSON at URL: {}", url)]
pub async fn get_json<T>(url: &str, context: &'static str)
    -> Result<T, anyhow::Error>
    where T: DeserializeOwned,
{
    log::info!("Fetching JSON at {}", url);
    let body_bytes = surf::get(url).await.ensure200(context)?
        .body_bytes().await.context(context)?;
    let jd = &mut serde_json::Deserializer::from_slice(&body_bytes);
    Ok(serde_path_to_error::deserialize(jd).context(context)?)
}

#[context("failed to fetch JSON at URL: {}", url)]
pub async fn get_json_opt<T>(url: &str, context: &'static str)
    -> Result<Option<T>, anyhow::Error>
    where T: DeserializeOwned,
{
    log::info!("Fetching optional JSON at {}", url);
    match surf::get(url).await {
        Ok(res) if res.status() == 404 => Ok(None),
        Ok(res) if res.status() != 200
            => Err(HttpFailure(res)).context(context),
        Ok(mut res) => {
            let body_bytes = res.body_bytes().await.context(context)?;
            let jd = &mut serde_json::Deserializer::from_slice(&body_bytes);
            Ok(serde_path_to_error::deserialize(jd).context(context)?)
        }
        Err(e) => Err(HttpError(e)).context(context),
    }
}

#[context("failed to download file at URL: {}", url)]
pub async fn get_file(dest: impl AsRef<Path>, url: &str)
    -> Result<(), anyhow::Error>
{
    let dest = dest.as_ref();
    log::info!("Downloading {} -> {}", url, dest.display());
    let response = surf::get(url).await.ensure200(url)?;
    let file = fs::File::create(dest).await
        .with_context(|| format!("writing {:?}", dest.display()))?;
    io::copy(response, file).await
        .with_context(|| format!("downloading {:?} -> {:?}",
                                 url, dest.display()))?;
    Ok(())
}
