use std::sync::Arc;
use std::time::Duration;

use async_std::future::{timeout, pending};
use async_std::prelude::FutureExt;
use rustls::client::ServerCertVerifier;

use edgedb_client::{Builder, Config};
use edgedb_client::errors::Error;
use edgedb_client::client::Connection;

use crate::hint::ArcError;


#[derive(Debug, Clone)]
pub struct Connector {
    params: Result<(Builder, Config), ArcError>,
}

impl Connector {
    pub fn new(builder: Result<Builder, anyhow::Error>) -> Connector {
        let params = builder.map_err(ArcError::from).and_then(|b| {
            b.build().map(|c| (b, c))
                .map_err(|e| ArcError::from(anyhow::anyhow!(e)))
        });
        Connector { params }
    }
    pub fn modify<F: FnOnce(&mut Builder)>(&mut self, f: F)
        -> anyhow::Result<&mut Self>
    {
        if let Ok((builder, cfg)) = self.params.as_mut() {
            f(builder);
            *cfg = builder.build()?;
        }
        Ok(self)
    }
    pub async fn connect(&self) -> Result<Connection, anyhow::Error> {
        let (_, cfg) = self.params.as_ref().map_err(Clone::clone)?;
        Ok(cfg.connect()
            .race(self.print_warning(cfg))
            .await?)
    }

    pub async fn connect_with_cert_verifier(
        &self, verifier: Arc<dyn ServerCertVerifier>
    ) -> anyhow::Result<Connection> {
        let (_, cfg) = self.params.as_ref().map_err(Clone::clone)?;
        cfg.connect_with_cert_verifier(verifier)
            .race(self.print_warning(cfg))
            .await.map_err(Into::into)
    }

    async fn print_warning(&self, cfg: &Config)
        -> Result<Connection, Error>
    {
        timeout(Duration::new(1, 0), pending::<()>()).await.ok();
        eprintln!("Connecting to an EdgeDB instance at {}...",
            cfg.display_addr());
        pending().await
    }
    pub fn get(&self) -> anyhow::Result<&Builder, ArcError> {
        let (builder, _) = self.params.as_ref().map_err(Clone::clone)?;
        Ok(builder)
    }
}
