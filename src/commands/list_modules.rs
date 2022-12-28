use crate::commands::Options;
use crate::commands::filter;
use crate::commands::list;
use crate::connect::Connection;


pub async fn list_modules(cli: &mut Connection, options: &Options,
    pattern: &Option<String>, case_sensitive: bool)
    -> Result<(), anyhow::Error>
{
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
    let items = filter::query(cli, &query, &pattern, case_sensitive).await?;
    list::print(items, "List of modules", options).await?;
    Ok(())
}
