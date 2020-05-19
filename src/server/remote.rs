use anyhow::Context;

use serde::de::DeserializeOwned;


#[derive(Debug, thiserror::Error)]
#[error("HTTP error: {0}")]
pub struct HttpError(surf::Error);

#[derive(Debug, thiserror::Error)]
#[error("HTTP failure: {} {}",
        self.0.status(), self.0.status().canonical_reason())]
pub struct HttpFailure(surf::Response);


trait HttpErrorExt<T> {
    fn context(self, text: &'static str) -> Result<T, anyhow::Error>;
}
trait HttpOkExt<T> {
    fn ensure200(self, text: &'static str) -> Result<T, anyhow::Error>;
}

impl HttpOkExt<surf::Response> for Result<surf::Response, surf::Error> {
    fn ensure200(self, text: &'static str)
        -> Result<surf::Response, anyhow::Error>
    {
        match self {
            Ok(res) if res.status() != 200
                   => Err(HttpFailure(res)).context(text),
            Ok(res) => return Ok(res),
            Err(e) => Err(HttpError(e)).context(text),
        }
    }
}

impl<T> HttpErrorExt<T> for Result<T, surf::Error> {
    fn context(self, text: &'static str) -> Result<T, anyhow::Error> {
        self.map_err(HttpError).context(text)
    }
}

pub async fn get_string(url: &str, context: &'static str)
    -> Result<String, anyhow::Error>
{
    log::info!("Fetching {}", url);
    Ok(surf::get(url).await.ensure200(context)?
        .body_string().await.context(context)?)
}

pub async fn get_json<T: DeserializeOwned>(url: &str, context: &'static str)
    -> Result<T, anyhow::Error>
{
    log::info!("Fetching JSON at {}", url);
    Ok(surf::get(url).await.ensure200(context)?
        .body_json::<T>().await.context(context)?)
}
