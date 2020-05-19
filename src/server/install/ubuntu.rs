use async_std::task;
use std::io;
use std::fs;
use std::str;

use crate::server::detect::Detect;
use crate::server::detect::linux::{OsInfo, UbuntuInfo};
use crate::server::install::{Operation, Command, Settings, KEY_FILE_URL};
use crate::server::remote;


fn sources_list_path(nightly: bool) -> &'static str {
    if nightly {
        "/etc/apt/sources.list.d/edgedb_server_install_nightly.list"
    } else {
        "/etc/apt/sources.list.d/edgedb_server_install.list"
    }
}

fn sources_list(codename: &str, nightly: bool) -> String {
    format!("deb https://packages.edgedb.com/apt {}{} main\n", codename,
        if nightly { ".nightly" } else { "" } )
}


pub fn prepare(settings: &Settings, _detect: &Detect,
    _os_info: &OsInfo, info: &UbuntuInfo)
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
    let sources_list = sources_list(&info.codename, settings.nightly);
    let list_path = sources_list_path(settings.nightly);
    let update_list = match fs::read(list_path) {
        Ok(data) => {
            str::from_utf8(&data).map(|x| x.trim()) != Ok(sources_list.trim())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!("Unable to read {} file: {}. Will replace...",
                list_path, e);
            true
        }
    };
    if update_list {
        operations.push(Operation::WritePrivilegedFile {
            path: list_path.into(),
            data: sources_list.into(),
        });
    }
    operations.push(Operation::PrivilegedCmd(
        Command::new("apt-get")
            .arg("update")
            .arg("--no-list-cleanup")
            .arg("-o")
                .arg(format!("Dir::Etc::sourcelist={}", list_path))
            .arg("-o").arg("Dir::Etc::sourceparts=-")
    ));
    operations.push(Operation::PrivilegedCmd(
        Command::new("apt-get")
        .arg("install")
        .arg("-y")
        // TODO(tailhook) version
        .arg(format!("{}-{}", settings.package_name, settings.major_version))
        .env("_EDGEDB_INSTALL_SKIP_BOOTSTRAP", "1")
    ));
    return Ok(operations);
}
