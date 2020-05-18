use std::io;
use std::fs;
use std::str;

use crate::server::detect::Detect;
use crate::server::detect::linux::{OsInfo, CentosInfo};
use crate::server::install::{Operation, Command, Settings, KEY_FILE_URL};


fn repo_file(nightly: bool) -> &'static str {
    if nightly {
        "/etc/yum.repos.d/edgedb-server-install-nightly.repo"
    } else {
        "/etc/yum.repos.d/edgedb-server-install.repo"
    }
}

fn repo_data(nightly: bool) -> String {
    format!("\
            [edgedb-server-install{name_suffix}]\n\
            name=edgedb-server-install{name_suffix}\n\
            baseurl=https://packages.edgedb.com/rpm/el$releasever{suffix}/\n\
            enabled=1\n\
            gpgcheck=1\n\
            gpgkey={keyfile}\n\
        ",
        name_suffix=if nightly { "-nightly" } else {""},
        suffix=if nightly { ".nightly" } else {""},
        keyfile=KEY_FILE_URL)
}


pub fn prepare(settings: &Settings,_detect: &Detect,
    _os_info: &OsInfo, _info: &CentosInfo)
    -> Result<Vec<Operation>, anyhow::Error>
{
    let mut operations = Vec::new();
    let repo_data = repo_data(settings.nightly);
    let repo_path = repo_file(settings.nightly);
    let update_list = match fs::read(&repo_path) {
        Ok(data) => {
            str::from_utf8(&data).map(|x| x.trim()) != Ok(repo_data.trim())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!("Unable to read {}: {}. Will replace...",
                repo_path, e);
            true
        }
    };
    if update_list {
        operations.push(Operation::WritePrivilegedFile {
            path: repo_path.into(),
            data: repo_data.into(),
        });
    }
    operations.push(Operation::PrivilegedCmd(
        Command::new("yum")
        .arg("-y")
        .arg("install")
        // TODO(tailhook) version
        .arg(format!("{}-{}", settings.package_name, settings.major_version))
    ));
    Ok(operations)
}
