use std::collections::HashMap;
use std::convert::TryInto;
use std::time::{Instant, Duration};
use std::mem::transmute;

use anyhow::Context;
use async_std::path::Path;
use async_std::fs;
use async_std::io::{self, Read, prelude::ReadExt};
use async_std::future::{timeout, pending};
use async_std::prelude::{FutureExt, StreamExt};
use bytes::{Bytes, BytesMut, BufMut};

use edgedb_protocol::client_message::{ClientMessage, Restore, RestoreBlock};
use edgedb_protocol::server_message::ServerMessage;
use edgedb_protocol::value::Value;
use crate::commands::Options;
use crate::commands::parser::{Restore as RestoreCmd};
use crate::print;
use crate::client::{Connection, Reader, Writer};

type Input = Box<dyn Read + Unpin + Send>;

const MAX_SUPPORTED_DUMP_VER: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PacketType {
    Header,
    Block,
}


async fn read_packet(input: &mut Input, expected: PacketType)
    -> Result<Option<Bytes>, anyhow::Error>
{
    let mut buf = [0u8; 1+20+4];
    let mut read = 0;
    while read < buf.len() {
        let n = input.read(&mut buf[read..]).await
            .context("Cannot read packet header")?;
        if n == 0 {  // EOF
            if read == 0 {
                return Ok(None);
            }
            Err(io::Error::from(io::ErrorKind::UnexpectedEof))
            .context("Cannot read packet header")?
        }
        read += n;
    }
    let typ = match buf[0] {
        b'H' => PacketType::Header,
        b'D' => PacketType::Block,
        _ => return Err(anyhow::anyhow!("Invalid block type {:x}", buf[0])),
    };
    if typ != expected {
        return Err(anyhow::anyhow!("Expected block {:?} got {:?}",
                    expected, typ));
    }
    let len = u32::from_be_bytes(buf[1+20..].try_into().unwrap()) as usize;
    let mut buf = BytesMut::with_capacity(len);
    unsafe {
        // this is safe because we use read_exact which initializes whole slice
        let dest: &mut [u8] = transmute(buf.bytes_mut());
        input.read_exact(dest).await
            .with_context(|| format!("Error reading block of {} bytes", len))?;
        buf.advance_mut(dest.len());
    }
    return Ok(Some(buf.freeze()));
}


async fn is_empty_db(cli: &mut Connection) -> Result<bool, anyhow::Error> {
    let mut query = cli.query::<i64>(r###"SELECT
            count(
                schema::Module
                FILTER NOT .builtin AND NOT .name = "default"
            ) + count(
                schema::Object
                FILTER .name LIKE "default::%"
            )
        "###, &Value::empty_tuple()).await?;
    let mut non_empty = false;
    while let Some(num) = query.next().await.transpose()? {
        if num > 0 {
            non_empty = true;
        }
    }
    return Ok(non_empty);
}

pub async fn restore<'x>(cli: &mut Connection, options: &Options,
    params: &RestoreCmd)
    -> Result<(), anyhow::Error>
{
    use PacketType::*;
    let RestoreCmd { allow_non_empty, file: ref filename, verbose } = *params;
    if !allow_non_empty {
        if is_empty_db(cli).await.context("Error checking DB emptyness")? {
            if options.command_line {
                return Err(anyhow::anyhow!("\
                    cannot restore: the database is not empty; \
                    consider using the --allow-non-empty option"));
            } else {
                return Err(anyhow::anyhow!(
                    "cannot restore: the database is not empty"));
            }
        }
    }

    // TODO(tailhook) check that DB is empty
    let file_ctx = &|| format!("Failed to read dump {}", filename.display());
    let mut input = if filename.to_str() == Some("-") {
        Box::new(io::stdin()) as Input
    } else {
        fs::File::open(filename).await
        .map(Box::new)
        .with_context(file_ctx)?
        as Input
    };
    let mut buf = [0u8; 17+8];
    input.read_exact(&mut buf).await
        .context("Cannot read header")
        .with_context(file_ctx)?;
    if &buf[..17] != b"\xFF\xD8\x00\x00\xD8EDGEDB\x00DUMP\x00" {
        Err(anyhow::anyhow!("File is not an edgedb dump"))
        .with_context(file_ctx)?
    }
    let version = i64::from_be_bytes(buf[17..].try_into().unwrap());
    if version == 0 || version > MAX_SUPPORTED_DUMP_VER {
        Err(anyhow::anyhow!("Unsupported dump version {}", version))
        .with_context(file_ctx)?
    }
    let header = read_packet(&mut input, Header).await.with_context(file_ctx)?
        .ok_or_else(|| anyhow::anyhow!("Dump is empty"))
                       .with_context(file_ctx)?;
    let start_headers = Instant::now();
    let mut seq = cli.start_sequence().await?;
    seq.send_messages(&[
        ClientMessage::Restore(Restore {
            headers: HashMap::new(),
            jobs: 1,
            data: header,
        })
    ]).await?;
    loop {
        let msg = seq.message().await?;
        match msg {
            ServerMessage::RestoreReady(_) => {
                if verbose {
                    eprintln!("Schema applied in {:?}",
                              start_headers.elapsed());
                }
                break;
            }
            ServerMessage::ErrorResponse(err) => {
                seq.err_sync().await.ok();
                return Err(anyhow::anyhow!(err)
                    .context("Error initiating restore protocol"));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "WARNING: unsolicited message {:?}", msg));
            }
        }
    }
    let result = send_blocks(&mut seq.writer, &mut input,
                             filename.as_ref(), verbose)
        .race(wait_response(&mut seq.reader, start_headers, verbose))
        .await;
    if let Err(..) = result {
        seq.err_sync().await.ok();
    } else {
        seq.end_clean();
    }
    result
}

async fn send_blocks(writer: &mut Writer<'_>, input: &mut Input,
    filename: &Path, verbose: bool)
    -> Result<(), anyhow::Error>
{
    use PacketType::*;

    let start_blocks = Instant::now();
    while
        let Some(data) = read_packet(input, Block).await
            .with_context(|| format!("Failed to read dump {}",
                                     filename.display()))?
    {
        writer.send_messages(&[
            ClientMessage::RestoreBlock(RestoreBlock { data })
        ]).await?;
    }
    writer.send_messages(&[ClientMessage::RestoreEof]).await?;
    if verbose {
        eprintln!("Blocks sent in {:?}", start_blocks.elapsed());
    }

    // This future should be canceled by wait_response() receiving
    // CommandComplete
    let start_waiting = Instant::now();
    loop {
        timeout(Duration::from_secs(60), pending()).await?;
        if verbose {
            eprintln!("Waiting for complete {:?}", start_waiting.elapsed());
        }
    }
}

async fn wait_response(reader: &mut Reader<'_>, start: Instant,
    verbose: bool)
    -> Result<(), anyhow::Error>
{
    loop {
        let msg = reader.message().await?;
        match msg {
            ServerMessage::CommandComplete(c) => {
                if verbose {
                    eprintln!("Complete in {:?}", start.elapsed());
                    print::completion(&c.status_data);
                }
                break;
            }
            ServerMessage::ErrorResponse(err) => {
                return Err(anyhow::anyhow!(err));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "WARNING: unsolicited message {:?}", msg));
            }
        }
    }
    Ok(())
}
