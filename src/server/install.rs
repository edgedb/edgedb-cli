use std::process::exit;

use crate::server::options::Install;
use crate::server::detect;

mod operation;
mod exit_codes;
mod settings;

// Distributions
mod centos;
mod debian;
mod ubuntu;


pub(in crate::server::install) use operation::{Operation, Command};
pub(in crate::server::install) use settings::{Settings, SettingsBuilder};

const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";


pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    let detect = detect::Detect::current_os();
    let settings_builder = SettingsBuilder::new(&detect, options)?;
    dbg!(&settings_builder);
    let settings = match settings_builder.build() {
        Ok(settings) => settings,
        Err(settings::BuildError::Fatal(e)) => Err(anyhow::anyhow!(e))?,
        Err(settings::BuildError::Configurable(errors)) => {
            for e in errors {
                eprintln!("{}", e);
            }
            return Err(anyhow::anyhow!("Add mentioned options or run with \
                --interactive (-i) for interactive mode"));
        }
    };
    settings.print();

    match &detect.os_info {
        detect::OsInfo::Linux(linux) => {
            let operations = match linux.get_distribution() {
                detect::linux::Distribution::Ubuntu(ubuntu) => {
                    ubuntu::prepare(&settings, &detect, linux, ubuntu)?
                }
                detect::linux::Distribution::Debian(debian) => {
                    debian::prepare(&settings, &detect, linux, debian)?
                }
                detect::linux::Distribution::Centos(centos) => {
                    centos::prepare(&settings, &detect, linux, centos)?
                }
                detect::linux::Distribution::Unknown => {
                    return Err(anyhow::anyhow!(
                        "Unsupported linux distribution. Supported: \
                        Debian, Ubuntu, Centos"));
                }
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
        detect::OsInfo::Windows(_) => {
            anyhow::bail!("Installation is unsupported on Windows yet");
        }
        detect::OsInfo::Macos(_) => {
            anyhow::bail!("Installation is unsupported on MacOS yet");
        }
        detect::OsInfo::Unknown => {
            anyhow::bail!("Cannot detect operationg system kind");
        }
    }
}
