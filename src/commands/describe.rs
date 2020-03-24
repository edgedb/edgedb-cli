use async_std::prelude::StreamExt;

use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::commands::helpers::quote_namespaced;
use crate::client::Client;


pub async fn describe<'x>(cli: &mut Client<'x>, _options: &Options,
    name: &str, verbose: bool)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<String>(
        &format!("DESCRIBE OBJECT {name} AS TEXT {flag}",
            name=quote_namespaced(name),
            flag=if verbose { "VERBOSE" } else {""}),
        &Value::empty_tuple(),
    ).await?;
    while let Some(name) = items.next().await.transpose()? {
        println!("{}", name);
    }
    Ok(())
}
