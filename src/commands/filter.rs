use std::borrow::Cow;


use crate::connect::Connection;
use edgedb_errors::Error;
use edgedb_protocol::QueryResult;


pub async fn query<R>(cli: &mut Connection, query: &str,
    pattern: &Option<String>, case_sensitive: bool)
    -> Result<Vec<R>, Error>
    where R: QueryResult
{
    if let Some(pat) = pattern {
        let pat = if case_sensitive {
            Cow::Borrowed(pat)
        } else {
            Cow::Owned(String::from("(?i)") + pat)
        };
        cli.query(query, &(&pat[..],)).await
    } else {
        cli.query(query, &()).await
    }
}
