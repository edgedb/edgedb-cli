use std::error;
use std::fmt;
use std::pin::Pin;

use anyhow::Context as _;
use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt};

use edgeql_parser::preparser::full_statement;

#[derive(Debug)]
pub struct EndOfFile;


pub async fn read_statement<T: AsyncRead>(buf: &mut BytesMut, stream: &mut T)
    -> anyhow::Result<Bytes>
    where T: Unpin
{
    let mut continuation = None;
    let statement_len = loop {
        match full_statement(&buf[..], continuation.take()) {
            Ok(len) => break len,
            Err(cont) => continuation = Some(cont),
        };
        buf.reserve(8192);
        let bytes_read = Pin::new(&mut *stream).read_buf(buf).await
            .context("error reading query")?;
        if bytes_read == 0 {
            if buf.iter().any(|x| !x.is_ascii_whitespace()) {
                return Ok(buf.split_to(buf.len()).freeze());
            }
            return Err(EndOfFile.into());
        }
    };
    let data = buf.split_to(statement_len).freeze();
    return Ok(data);
}

impl fmt::Display for EndOfFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        "end of file".fmt(f)
    }
}

impl error::Error for EndOfFile {
}
