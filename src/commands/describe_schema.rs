use async_std::prelude::StreamExt;

use edgedb_protocol::value::Value;
use crate::commands::Options;
use edgedb_client::client::Connection;
use crate::highlight;


pub async fn describe_schema(cli: &mut Connection, options: &Options)
    -> Result<(), anyhow::Error>
{
    let mut items = cli.query::<String>(
        "DESCRIBE SCHEMA AS SDL",
        &Value::empty_tuple(),
    ).await?;
    while let Some(text) = items.next().await.transpose()? {
        if let Some(ref styler) = options.styler {
            let mut out = String::with_capacity(text.len());
            highlight::edgeql(&mut out, &text, styler);
            println!("{}", out);
        } else {
            println!("{}", text);
        }
    }
    Ok(())
}
