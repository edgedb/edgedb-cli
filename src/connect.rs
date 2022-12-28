use std::time::Duration;
use std::sync::Arc;
use std::future::{Future, pending};

use bytes::Bytes;
use tokio::time::sleep;

use edgedb_protocol::model::Uuid;
use edgedb_errors::{Error, ErrorKind, ResultExt};
use edgedb_errors::{NoDataError};
use edgedb_protocol::QueryResult;
use edgedb_protocol::common::Capabilities;
use edgedb_protocol::descriptors::RawTypedesc;
use edgedb_protocol::features::ProtocolVersion;
use edgedb_protocol::query_arg::QueryArgs;
use edgedb_protocol::server_message::TransactionState;
use edgedb_protocol::client_message::State;
use edgedb_protocol::value::Value;
use edgedb_tokio::raw;
use edgedb_tokio::server_params::ServerParam;
use edgedb_tokio::{Builder, Config};

use crate::hint::ArcError;
use crate::portable::ver;


#[derive(Debug, Clone)]
pub struct Connector {
    params: Result<(Builder, Config), ArcError>,
}

pub struct Connection {
    inner: raw::Connection,
    server_version: Option<ver::Build>,
    state: State,
}

trait AssertConn: Send + 'static {}
impl AssertConn for Connection {}

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
        let conn = tokio::select!(
            conn = Connection::connect(&cfg) => conn?,
            _ = self.print_warning(cfg) => unreachable!(),
        );
        Ok(conn)
    }

    async fn print_warning(&self, cfg: &Config)
        -> Result<Connection, Error>
    {
        sleep(Duration::new(1, 0)).await;
        eprintln!("Connecting to an EdgeDB instance at {}...",
            cfg.display_addr());
        pending().await
    }
    pub fn get(&self) -> anyhow::Result<&Builder, ArcError> {
        let (builder, _) = self.params.as_ref().map_err(Clone::clone)?;
        Ok(builder)
    }
}

impl Connection {
    pub async fn connect(cfg: &Config) -> Result<Connection, Error> {
        Ok(Connection {
            inner: raw::Connection::connect(&cfg).await?,
            state: State::empty(),
            server_version: None,
        })
    }
    pub async fn get_version(&mut self) -> Result<&ver::Build, Error> {
        if self.server_version.is_some() {
            return Ok(self.server_version.as_ref().unwrap());
        }
        let resp: raw::Response<String> = self.inner.query_required_single(
            "SELECT sys::get_version_as_str()", &(),
            &State::empty(),
            Capabilities::empty(),
        ).await.context("cannot fetch database version")?;
        let build = resp.data.parse()?;
        Ok(self.server_version.insert(build))
    }
    pub async fn query<R, A>(&mut self, query: &str, arguments: &A)
        -> Result<Vec<R>, Error>
        where A: QueryArgs,
              R: QueryResult,
    {
        let resp = self.inner.query(
            query, arguments, &self.state, Capabilities::ALL,
        ).await?;
        self.process_response(&resp)?;
        return Ok(resp.data);
    }
    pub async fn query_single<R, A>(&mut self, query: &str, arguments: &A)
        -> Result<Option<R>, Error>
        where A: QueryArgs,
              R: QueryResult,
    {
        let resp = self.inner.query_single(
            query, arguments, &self.state, Capabilities::ALL,
        ).await?;
        self.process_response(&resp)?;
        return Ok(resp.data);
    }
    pub async fn query_required_single<R, A>(
        &mut self, query: &str, arguments: &A)
        -> Result<R, Error>
        where A: QueryArgs,
              R: QueryResult,
    {
        let res = self.query_single(query, arguments).await?;
        return res.ok_or_else(|| NoDataError::with_message(
            "query row returned zero results"));
    }
    pub async fn execute<A>(&mut self, query: &str, arguments: &A)
        -> Result<Bytes, Error>
        where A: QueryArgs,
    {
        let resp = self.inner.execute(
            query, arguments, &self.state, Capabilities::ALL
        ).await?;
        self.process_response(&resp)?;
        return Ok(resp.status_data);
    }
    pub fn get_server_param<T: ServerParam>(&self) -> Option<&T::Value> {
        self.get_server_param::<T>()
    }
    pub fn is_consistent(&self) -> bool {
        self.inner.is_consistent()
    }
    pub async fn ping_while<T, F>(&mut self, other: F) -> T
        where F: Future<Output = T>
    {
        self.inner.ping_while(other).await
    }
    pub async fn terminate(self) -> Result<(), Error> {
        self.inner.terminate().await
    }
    pub fn protocol(&self) -> &ProtocolVersion {
        self.inner.protocol()
    }
    pub fn transaction_state(&self) -> TransactionState {
        self.inner.transaction_state()
    }
    fn process_response<T>(&mut self, resp: &raw::Response<T>)
        -> Result<(), Error>
    {
        if let Some(raw_state) = &resp.new_state {
            self.state = raw_state.clone();
        }
        Ok(())
    }
    pub fn get_state_as_value(&self) -> (Uuid, Value) {
        todo!();
    }
    pub fn get_state(&self) -> &State {
        &self.state
    }
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }
    pub fn get_state_desc(&self) -> RawTypedesc {
        self.inner.state_descriptor().clone()
    }
}
