use std::borrow::Cow;


use edgedb_client::client::Connection;
use edgedb_client::reader::QueryResponse;
use edgedb_client::errors::Error;
use edgedb_protocol::QueryResult;


pub async fn query<'x, R>(cli: &'x mut Connection, query: &str,
    pattern: &Option<String>, case_sensitive: bool)
    -> Result<QueryResponse<'x, R>, Error>
    where R: QueryResult
{
    if let Some(pat) = pattern {
        let pat = if case_sensitive {
            Cow::Borrowed(pat)
        } else {
            Cow::Owned(String::from("(?i)") + pat)
        };
        cli.query(&query, &(&pat[..],)).await
    } else {
        cli.query(&query, &()).await
    }
}
