use std::time::Duration;

use async_std::future::{timeout, pending};
use async_std::prelude::FutureExt;

use edgedb_client::Builder;
use edgedb_client::client::Connection;


#[derive(Debug, Clone)]
pub struct Connector {
    params: Builder,
}

impl std::ops::Deref for Connector {
    type Target = Builder;
    fn deref(&self) -> &Builder {
        &self.params
    }
}

impl std::ops::DerefMut for Connector {
    fn deref_mut(&mut self) -> &mut Builder {
        &mut self.params
    }
}

impl Connector {
    pub fn new(params: Builder) -> Connector {
        Connector { params }
    }
    pub async fn connect(&self) -> Result<Connection, anyhow::Error> {
        return self.params.connect()
            .race(self.print_warning())
            .await
    }

    async fn print_warning(&self) -> Result<Connection, anyhow::Error> {
        timeout(Duration::new(1, 0), pending::<()>()).await.ok();
        eprintln!("Connecting to an EdgeDB instance at {}...",
            self.params.get_addr());
        pending().await
    }
}
