use std::sync::Arc;
use std::time::Duration;

use async_std::future::{timeout, pending};
use async_std::prelude::FutureExt;
use rustls::ServerCertVerifier;

use edgedb_client::Builder;
use edgedb_client::client::Connection;

use crate::hint::ArcError;


#[derive(Debug, Clone)]
pub struct Connector {
    params: Result<Builder, ArcError>,
}

impl Connector {
    pub fn new(params: Result<Builder, anyhow::Error>) -> Connector {
        Connector { params: params.map_err(ArcError::from) }
    }
    pub fn modify<F: FnOnce(&mut Builder)>(&mut self, f: F) -> &mut Self {
        self.params.as_mut().map(f).ok();
        self
    }
    pub async fn connect(&self) -> Result<Connection, anyhow::Error> {
        let params = self.params.as_ref().map_err(Clone::clone)?;
        return params.connect()
            .race(self.print_warning(params))
            .await
    }

    pub async fn connect_with_cert_verifier(
        &self, verifier: Arc<dyn ServerCertVerifier>
    ) -> anyhow::Result<Connection> {
        let params = self.params.as_ref().map_err(Clone::clone)?;
        params.connect_with_cert_verifier(verifier)
            .race(self.print_warning(params))
            .await
    }

    async fn print_warning(&self, params: &Builder)
        -> Result<Connection, anyhow::Error>
    {
        timeout(Duration::new(1, 0), pending::<()>()).await.ok();
        eprintln!("Connecting to an EdgeDB instance at {}...",
            params.get_addr());
        pending().await
    }
    pub fn get(&self) -> anyhow::Result<&Builder, ArcError> {
        self.params.as_ref().map_err(Clone::clone)
    }
}
