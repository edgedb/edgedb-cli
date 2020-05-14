use std::io;
use std::fs;
use std::str;

use crate::server::detect::Detect;
use crate::server::detect::linux::{OsInfo, CentosInfo};
use crate::server::install::{Operation, Command};
use crate::server::options::Install;


const REPO_FILE_PATH: &str = "/etc/yum.repos.d/edgedb-server-install.repo";
const REPO_FILE_DATA: &str =
    "\
        [edgedb-server-install]\n\
        name=edgedb-server-install\n\
        baseurl=https://packages.edgedb.com/rpm/el$releasever/\n\
        enabled=1\n\
        gpgcheck=1\n\
        gpgkey=https://packages.edgedb.com/keys/edgedb.asc\n\
    ";


pub fn prepare(_options: &Install, _detect: &Detect,
    _os_info: &OsInfo, _info: &CentosInfo)
    -> Result<Vec<Operation>, anyhow::Error>
{
    let mut operations = Vec::new();
    let update_list = match fs::read(REPO_FILE_PATH) {
        Ok(data) => {
            str::from_utf8(&data).map(|x| x.trim()) != Ok(REPO_FILE_DATA)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            log::warn!(
                "Unable to read {}: {}. Will replace...", REPO_FILE_PATH, e);
            true
        }
    };
    if update_list {
        operations.push(Operation::WritePrivilegedFile {
            path: REPO_FILE_PATH.into(),
            data: REPO_FILE_DATA.into(),
        });
    }
    operations.push(Operation::PrivilegedCmd(
        Command::new("yum")
        .arg("-y")
        .arg("install")
        // TODO(tailhook) version
        .arg("edgedb-1-alpha2")
    ));
    return Ok(operations);
}
