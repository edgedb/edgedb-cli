use std::process::exit;

use crate::server::options::Install;
use crate::server::detect;

mod ubuntu;
mod operation;
mod exit_codes;

pub use operation::{Operation, Command};

const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";


pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    let detect = detect::Detect::current_os();
    match &detect.os_info {
        detect::OsInfo::Linux(linux) => {
            let operations = match linux.get_distribution() {
                detect::linux::Distribution::Ubuntu(ubuntu) => {
                    ubuntu::prepare(options, &detect, linux, ubuntu)?
                }
                _ => todo!(),
            };
            let mut ctx = operation::Context::new();
            let has_privileged = operations.iter().any(|x| x.is_privileged());
            if has_privileged && linux.get_user_id() != 0 {
                println!("The following commands will be run with elevated \
                    privileges using sudo:");
                for op in &operations {
                    if op.is_privileged() {
                        println!("    {}", op.format(true));
                    }
                }
                println!("Depending on system settings sudo may now ask \
                          you for your password...");
                match linux.get_sudo_path() {
                    Some(cmd) => ctx.set_elevation_cmd(cmd),
                    None => {
                        eprintln!("`sudo` command not found. \
                                   Cannot elevate acquire needed for \
                                   installation. Please run \
                                   `edgedb server install` as root user.");
                        exit(exit_codes::NO_SUDO);
                    }
                }
            }
            for op in &operations {
                op.perform(&ctx)?;
            }
            Ok(())
        }
        _ => todo!(),
    }
}
