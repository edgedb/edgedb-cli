use async_std::task;
use std::io;
use std::fs;
use std::str;

use crate::server::detect::Detect;
use crate::server::detect::linux::{OsInfo, DebianInfo};
use crate::server::install::{Operation, Command, KEY_FILE_URL};
use crate::server::options::Install;
use crate::server::remote;


const SOURCES_LIST_PATH: &str =
    "/etc/apt/sources.list.d/edgedb_server_install.list";


fn sources_list(codename: &str) -> String {
    format!("deb https://packages.edgedb.com/apt {} main\n", codename)
}


pub fn prepare(_options: &Install, _detect: &Detect,
    _os_info: &OsInfo, info: &DebianInfo)
    -> Result<Vec<Operation>, anyhow::Error>
{
    let key = task::block_on(remote::get_string(KEY_FILE_URL,
        "downloading key file"))?;
    let mut operations = Vec::new();
    operations.push(Operation::FeedPrivilegedCmd {
        input: key.into(),
        cmd: Command::new("apt-key")
            .arg("add")
            .arg("-"),
    });
    let sources_list = sources_list(&info.codename);
    let update_list = match fs::read(SOURCES_LIST_PATH) {
        Ok(data) => {
            str::from_utf8(&data).map(|x| x.trim()) != Ok(sources_list.trim())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!(
                "Unable to read {} file: {}. Will replace...",
                    SOURCES_LIST_PATH, e);
            true
        }
    };
    if update_list {
        operations.push(Operation::WritePrivilegedFile {
            path: SOURCES_LIST_PATH.into(),
            data: sources_list.into(),
        });
    }
    operations.push(Operation::PrivilegedCmd(
        Command::new("apt-get")
            .arg("update")
            .arg("--no-list-cleanup")
            .arg("-o")
                .arg(format!("Dir::Etc::sourcelist={}", SOURCES_LIST_PATH))
            .arg("-o").arg("Dir::Etc::sourceparts=-")
    ));
    operations.push(Operation::PrivilegedCmd(
        Command::new("apt-get")
        .arg("install")
        .arg("-y")
        // TODO(tailhook) version
        .arg("edgedb-1-alpha2")
    ));
    return Ok(operations);
}
