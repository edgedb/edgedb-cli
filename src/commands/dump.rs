use std::default::Default;

use anyhow::Context;
use async_std::path::{Path, PathBuf};
use async_std::stream::StreamExt;
use async_std::fs;
use async_std::io::{self, Write, prelude::WriteExt};

use edgedb_protocol::client_message::{ClientMessage, Dump};
use edgedb_protocol::server_message::ServerMessage;
use edgedb_protocol::value::Value;
use edgedb_client::client::Connection;

use crate::platform::tmp_file_name;
use crate::commands::Options;
use crate::commands::list_databases::get_databases;
use crate::commands::parser::{Dump as DumpOptions, DumpFormat};


type Output = Box<dyn Write + Unpin + Send>;


pub struct Guard {
    filenames: Option<(PathBuf, PathBuf)>,
}


impl Guard {
    async fn open(filename: &Path) -> anyhow::Result<(Output, Guard)> {
        if filename.to_str() == Some("-") {
            Ok((Box::new(io::stdout()), Guard { filenames: None }))
        } else if cfg!(windows)
            || filename.starts_with("/dev/")
            || filename.file_name().is_none()
        {
            let file = fs::File::create(&filename).await
                .context(filename.display().to_string())?;
            Ok((Box::new(file), Guard { filenames: None }))
        } else {
            let tmp_path = filename.with_file_name(
                tmp_file_name(filename.as_ref()));
            if tmp_path.exists().await {
                fs::remove_file(&tmp_path).await.ok();
            }
            let tmp_file = fs::File::create(&tmp_path).await
                .context(tmp_path.display().to_string())?;
            Ok((Box::new(tmp_file), Guard {
                filenames: Some((tmp_path, filename.to_owned())),
            }))
        }
    }
    async fn commit(self) -> anyhow::Result<()> {
        if let Some((tmp_filename, filename)) = self.filenames {
            fs::rename(tmp_filename, filename).await?;
        }
        Ok(())
    }
}


pub async fn dump(cli: &mut Connection, general: &Options,
    options: &DumpOptions)
    -> Result<(), anyhow::Error>
{
    if options.all {
        if let Some(dformat) = options.format {
            if dformat != DumpFormat::Dir {
                anyhow::bail!("only `--format=dir` is supported for `--all`");
            }
        } else {
            anyhow::bail!("`--format=dir` is required when using `--all`");
        }
        dump_all(cli, general, options.path.as_ref()).await
    } else {
        if options.format.is_some() {
            anyhow::bail!("`--format` is reserved for dump using `--all`");
        }
        dump_db(cli, general, options.path.as_ref()).await
    }
}

async fn dump_db(cli: &mut Connection, _options: &Options, filename: &Path)
    -> Result<(), anyhow::Error>
{
    let mut seq = cli.start_sequence().await?;
    let (mut output, guard) = Guard::open(filename).await?;
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
        ServerMessage::ErrorResponse(err) => {
            seq.err_sync().await.ok();
            return Err(anyhow::anyhow!(err)
                .context("Error receiving dump header"));
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
            ServerMessage::ErrorResponse(err) => {
                seq.err_sync().await.ok();
                return Err(anyhow::anyhow!(err)
                    .context("Error receiving dump block"));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "WARNING: unsolicited message {:?}", msg));
            }
        }
    }
    guard.commit().await?;
    Ok(())
}

async fn get_text(cli: &mut Connection, query: &str)
    -> Result<String, anyhow::Error>
{
    let mut response = cli.query(query, &Value::empty_tuple()).await?;
    let text = loop {
        if let Some(text) = response.next().await.transpose()? {
            break text;
        } else {
            anyhow::bail!("`{}` returned an empty value", query.escape_default());
        }
    };
    anyhow::ensure!(matches!(response.next().await, None),
        "`{}` returned more than one value", query.escape_default());
    Ok(text)
}

pub async fn dump_all(cli: &mut Connection, options: &Options, dir: &Path)
    -> Result<(), anyhow::Error>
{
    let databases = get_databases(cli).await?;
    let config = get_text(cli, "DESCRIBE SYSTEM CONFIG").await?;
    let roles = get_text(cli, "DESCRIBE ROLES").await?;

    fs::create_dir_all(dir).await?;

    let (mut init, guard) = Guard::open(&dir.join("init.edgeql")).await?;
    if !config.trim().is_empty() {
        init.write_all(b"# DESCRIBE SYSTEM CONFIG\n").await?;
        init.write_all(config.as_bytes()).await?;
        init.write_all(b"\n").await?;
    }
    if !roles.trim().is_empty() {
        init.write_all(b"# DESCRIBE ROLES\n").await?;
        init.write_all(roles.as_bytes()).await?;
        init.write_all(b"\n").await?;
    }
    guard.commit().await?;

    let mut conn_params = options.conn_params.clone();
    for database in &databases {
        let mut db_conn = conn_params
            .modify(|p| { p.database(database); })
            .connect().await?;
        let filename = dir.join(urlencoding::encode(database) + ".dump");
        dump_db(&mut db_conn, options, &filename).await?;
    }

    Ok(())
}
