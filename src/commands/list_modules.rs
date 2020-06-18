use crate::commands::Options;
use crate::commands::filter;
use crate::commands::list;
use crate::client::Connection;


pub async fn list_modules(cli: &mut Connection, options: &Options,
    pattern: &Option<String>, case_sensitive: bool)
    -> Result<(), anyhow::Error>
{
    let pat = filter::pattern_to_value(pattern, case_sensitive);
    let filter = if pattern.is_some() {
        "FILTER re_test(<str>$0, name)"
    } else {
        ""
    };
    let query = format!(r###"
        SELECT name := schema::Module.name
        {filter}
        ORDER BY name
    "###, filter=filter);
    let items = cli.query(&query, &pat).await?;
    list::print(items, "List of modules", options).await?;
    Ok(())
}
