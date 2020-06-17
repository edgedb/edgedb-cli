use std::ffi::OsString;
use std::default::Default;

use anyhow::Context;
use async_std::path::Path;
use async_std::fs;
use async_std::io::{self, Write, prelude::WriteExt};

use edgedb_protocol::client_message::{ClientMessage, Dump};
use edgedb_protocol::server_message::ServerMessage;
use crate::commands::Options;
use crate::client::Connection;

type Output = Box<dyn Write + Unpin + Send>;


pub async fn dump(cli: &mut Connection, _options: &Options, filename: &Path)
    -> Result<(), anyhow::Error>
{
    let mut seq = cli.start_sequence().await?;
    let (mut output, tmp_filename) = if filename.to_str() == Some("-") {
        (Box::new(io::stdout()) as Output, None)
    } else if cfg!(windows)
        || filename.starts_with("/dev/")
        || filename.file_name().is_none()
    {
        let file = fs::File::create(&filename).await
            .context(filename.display().to_string())?;
        (Box::new(file) as Output, None)
    } else {
        let name = filename.file_name().unwrap();
        let mut tmp_name = OsString::with_capacity(name.len() + 10);
        tmp_name.push(".");
        tmp_name.push(name);
        tmp_name.push(".edb.tmp");
        let tmp_filename = filename.with_file_name(tmp_name);
        if tmp_filename.exists().await {
            fs::remove_file(&tmp_filename).await.ok();
        }
        let tmpfile = fs::File::create(&tmp_filename).await
            .context(tmp_filename.display().to_string())?;
        (Box::new(tmpfile) as Output, Some(tmp_filename))
    };
    output.write_all(
        b"\xFF\xD8\x00\x00\xD8EDGEDB\x00DUMP\x00\
          \x00\x00\x00\x00\x00\x00\x00\x01"
        ).await?;

    seq.send_messages(&[
        ClientMessage::Dump(Dump {
            headers: Default::default(),
        }),
        ClientMessage::Sync,
    ]).await?;

    let mut header_buf = Vec::with_capacity(25);
    let msg = seq.message().await?;
    match msg {
        ServerMessage::DumpHeader(packet) => {
            // this is ensured because length in the protocol is u32 too
            assert!(packet.data.len() <= u32::max_value() as usize);

            header_buf.truncate(0);
            header_buf.push(b'H');
            header_buf.extend(
                &sha1::Sha1::from(&packet.data).digest().bytes()[..]);
            header_buf.extend(
                &(packet.data.len() as u32).to_be_bytes()[..]);
            output.write_all(&header_buf).await?;
            output.write_all(&packet.data).await?;
        }
        _ => {
            return Err(anyhow::anyhow!(
                "WARNING: unsolicited message {:?}", msg));
        }
    }
    loop {
        let msg = seq.message().await?;
        match msg {
            ServerMessage::CommandComplete(..) => {
                seq.expect_ready().await?;
                break;
            }
            ServerMessage::DumpBlock(packet) => {
                // this is ensured because length in the protocol is u32 too
                assert!(packet.data.len() <= u32::max_value() as usize);

                header_buf.truncate(0);
                header_buf.push(b'D');
                header_buf.extend(
                    &sha1::Sha1::from(&packet.data).digest().bytes()[..]);
                header_buf.extend(
                    &(packet.data.len() as u32).to_be_bytes()[..]);
                output.write_all(&header_buf).await?;
                output.write_all(&packet.data).await?;
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "WARNING: unsolicited message {:?}", msg));
            }
        }
    }
    if let Some(tmp_filename) = tmp_filename {
        fs::rename(tmp_filename, filename).await?;
    }
    Ok(())
}
