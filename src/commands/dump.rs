use std::path::{Path, PathBuf};

use anyhow::Context;
use indicatif::{ProgressBar, HumanBytes};
use tokio::fs;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use sha1::Digest;

use tokio_stream::StreamExt;

use crate::commands::Options;
use crate::commands::list_databases::get_databases;
use crate::commands::parser::{Dump as DumpOptions, DumpFormat};
use crate::connect::Connection;
use crate::platform::tmp_file_name;


type Output = Box<dyn AsyncWrite + Unpin + Send>;


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
            if fs::metadata(&tmp_path).await.is_ok() {
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
        dump_all(cli, general, options.path.as_ref(), options.include_secrets).await
    } else {
        if options.format.is_some() {
            anyhow::bail!("`--format` is reserved for dump using `--all`");
        }
        dump_db(cli, general, options.path.as_ref(), options.include_secrets).await
    }
}

async fn dump_db(cli: &mut Connection, _options: &Options, filename: &Path,
                 mut include_secrets: bool)
    -> Result<(), anyhow::Error>
{
    if cli.get_version().await?.specific() < "4.0-alpha.2".parse().unwrap() {
        include_secrets = false;
    }

    let dbname = cli.database().to_string();
    eprintln!("Starting dump for {dbname}...");

    let (mut output, guard) = Guard::open(filename).await?;
    output.write_all(
        b"\xFF\xD8\x00\x00\xD8EDGEDB\x00DUMP\x00\
          \x00\x00\x00\x00\x00\x00\x00\x01"
        ).await?;

    let (header, mut blocks) = cli.dump(include_secrets).await?;

    // this is ensured because length in the protocol is u32 too
    assert!(header.data.len() <= u32::MAX as usize);

    let mut header_buf = Vec::with_capacity(25);

    header_buf.push(b'H');
    header_buf.extend(&sha1::Sha1::new_with_prefix(&header.data).finalize()[..]);
    header_buf.extend(&(header.data.len() as u32).to_be_bytes()[..]);
    output.write_all(&header_buf).await?;
    output.write_all(&header.data).await?;

    let bar = ProgressBar::new_spinner();
    let mut processed = 0;

    while let Some(packet) = blocks.next().await.transpose()? {
        let packet_length = packet.data.len();
        bar.tick();
        processed += packet_length;
        bar.set_message(format!("Database {dbname} dump: {} processed.", HumanBytes(processed as u64)));
        bar.message();

        // this is ensured because length in the protocol is u32 too
        assert!(packet_length <= u32::MAX as usize);

        header_buf.truncate(0);
        header_buf.push(b'D');
        header_buf.extend(&sha1::Sha1::new_with_prefix(&packet.data).finalize()[..]);
        header_buf.extend(&(packet_length as u32).to_be_bytes()[..]);
        output.write_all(&header_buf).await?;
        output.write_all(&packet.data).await?;
    }
    guard.commit().await?;
    bar.abandon_with_message(format!("Finished dump for {dbname}. Total size: {}", HumanBytes(processed as u64)));
    Ok(())
}

pub async fn dump_all(cli: &mut Connection, options: &Options, dir: &Path,
                      include_secrets: bool)
    -> Result<(), anyhow::Error>
{
    let databases = get_databases(cli).await?;
    let config: String = cli.query_required_single("DESCRIBE SYSTEM CONFIG", &()).await?;
    let roles: String = cli.query_required_single("DESCRIBE ROLES", &()).await?;

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
            .database(database)?
            .connect().await?;
        let filename = dir.join(&(urlencoding::encode(database) + ".dump")[..]);
        dump_db(&mut db_conn, options, &filename, include_secrets).await?;
    }

    Ok(())
}
