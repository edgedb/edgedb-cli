use std::error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::slice;
use std::task::{Poll, Context};

use anyhow;
use async_std::io::{Read as AsyncRead};
use bytes::{Bytes, BytesMut, BufMut};

use edgeql_parser::preparser::{full_statement, Continuation};


#[derive(Debug)]
pub struct EndOfFile;

pub struct ReadStatement<'a, T> {
    buf: &'a mut BytesMut,
    eof: bool,
    stream: &'a mut T,
    continuation: Option<Continuation>,
}


impl<'a, T> ReadStatement<'a, T> {
    pub fn new(buf: &'a mut BytesMut, stream: &'a mut T)
        -> ReadStatement<'a, T>
    {
        ReadStatement { buf, stream, continuation: None, eof: false }
    }
}

impl<'a, T> Future for ReadStatement<'a, T>
    where T: AsyncRead + Unpin,
{
    type Output = Result<Bytes, anyhow::Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let ReadStatement {
            buf, stream, ref mut continuation, ref mut eof
        } = &mut *self;
        if *eof {
            return Poll::Ready(Err(EndOfFile.into()));
        }
        let statement_len = loop {
            match full_statement(&buf[..], continuation.take()) {
                Ok(len) => break len,
                Err(cont) => *continuation = Some(cont),
            };
            buf.reserve(8192);
            unsafe {
                // this is safe because the underlying ByteStream always
                // initializes read bytes
                let chunk = buf.chunk_mut();
                let dest: &mut [u8] = slice::from_raw_parts_mut(
                    chunk.as_mut_ptr(), chunk.len());
                match Pin::new(&mut *stream).poll_read(cx, dest) {
                    Poll::Ready(Ok(0)) => {
                        *eof = true;
                        if buf.iter().any(|x| !x.is_ascii_whitespace()) {
                            return Poll::Ready(Ok(
                                buf.split_to(buf.len()).freeze()));
                        }
                        return Poll::Ready(Err(EndOfFile.into()));
                    }
                    Poll::Ready(Ok(bytes)) => {
                        buf.advance_mut(bytes);
                        continue;
                    }
                    Poll::Ready(err @ Err(_)) => { err?; }
                    Poll::Pending => return Poll::Pending,
                }
            }
        };
        let data = buf.split_to(statement_len).freeze();
        return Poll::Ready(Ok(data));
    }
}

impl fmt::Display for EndOfFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        "end of file".fmt(f)
    }
}

impl error::Error for EndOfFile {
}
