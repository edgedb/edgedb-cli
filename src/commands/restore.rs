use std::collections::HashMap;
use std::convert::TryInto;
use std::time::{Instant, Duration};
use std::mem::transmute;

use anyhow::Context;
use async_std::path::Path;
use async_std::fs;
use async_std::io::{self, Read, prelude::ReadExt};
use async_std::future::{timeout, pending};
use async_std::prelude::{FutureExt};
use async_listen::ByteStream;
use bytes::{Bytes, BytesMut, BufMut};

use edgedb_protocol::client_message::{ClientMessage, Restore, RestoreBlock};
use edgedb_protocol::server_message::ServerMessage;
use crate::commands::Options;
use crate::commands::helpers::print_result;
use crate::client::{Client, Reader, Writer};

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


pub async fn restore<'x>(cli: &mut Client<'x>, _options: &Options,
    filename: &Path, allow_non_empty: bool)
    -> Result<(), anyhow::Error>
{
    use PacketType::*;

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
    cli.send_messages(&[
        ClientMessage::Restore(Restore {
            headers: HashMap::new(),
            jobs: 1,
            data: header,
        })
    ]).await?;
    loop {
        let msg = cli.reader.message().await?;
        match msg {
            ServerMessage::RestoreReady(_) => {
                eprintln!("Schema applied in {:?}", start_headers.elapsed());
                break;
            }
            ServerMessage::ErrorResponse(err) => {
                cli.err_sync().await.ok();
                return Err(anyhow::anyhow!(err)
                    .context("Error initiating restore protocol"));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "WARNING: unsolicited message {:?}", msg));
            }
        }
    }
    let result = send_blocks(&mut cli.writer, &mut input, filename)
        .race(wait_response(&mut cli.reader, start_headers))
        .await;
    if let Err(..) = result {
        cli.err_sync().await.ok();
    }
    result
}

async fn send_blocks(writer: &mut Writer<'_>, input: &mut Input,
    filename: &Path)
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
    eprintln!("Blocks sent in {:?}", start_blocks.elapsed());

    // This future should be canceled by wait_response() receiving
    // CommandComplete
    let start_waiting = Instant::now();
    loop {
        timeout(Duration::from_secs(60), pending()).await?;
        eprintln!("Waiting for complete {:?}", start_waiting.elapsed());
    }
}

async fn wait_response(reader: &mut Reader<&'_ ByteStream>, start: Instant)
    -> Result<(), anyhow::Error>
{
    loop {
        let msg = reader.message().await?;
        match msg {
            ServerMessage::CommandComplete(c) => {
                eprintln!("Complete in {:?}", start.elapsed());
                print_result(c.status_data);
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
