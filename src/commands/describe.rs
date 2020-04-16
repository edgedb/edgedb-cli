use async_std::prelude::StreamExt;

use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::commands::helpers::quote_namespaced;
use crate::client::Client;
use crate::highlight;


pub async fn describe<'x>(cli: &mut Client<'x>, options: &Options,
    name: &str, verbose: bool)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<String>(
        &format!("DESCRIBE OBJECT {name} AS TEXT {flag}",
            name=quote_namespaced(name),
            flag=if verbose { "VERBOSE" } else {""}),
        &Value::empty_tuple(),
    ).await?;
    while let Some(text) = items.next().await.transpose()? {
        if let Some(ref styler) = options.styler {
            println!("{}", highlight::edgeql(&text, styler));
        } else {
            println!("{}", text);
        }
    }
    Ok(())
}
