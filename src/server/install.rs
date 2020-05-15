use std::process::exit;

use semver::Version;

use crate::server::options::Install;
use crate::server::detect;

mod operation;
mod exit_codes;

// Distributions
mod centos;
mod debian;
mod ubuntu;


pub use operation::{Operation, Command};

const KEY_FILE_URL: &str = "https://packages.edgedb.com/keys/edgedb.asc";

pub struct VersionDirectory {
    current_version: Version,
    nightly_version: Version,
}

pub struct VersionInfo {
    package_suffix: String,
    nightly: bool,
    package_name: String,
}

fn package_name(v: &Version) -> String {
    if v <= &Version::parse("1.0.0-alpha2").unwrap() {
        "edgedb".into()
    } else {
        "edgedb-server".into()
    }
}

fn package_suffix(v: &Version) -> String {
    use std::fmt::Write;

    let mut ver = if v.minor > 0 {
        format!("{}-{}", v.major, v.minor)
    } else {
        format!("{}", v.major)
    };
    for item in &v.pre {
        write!(&mut ver, "-{}", item).unwrap();
    }
    ver
}

pub fn install(options: &Install) -> Result<(), anyhow::Error> {
    let detect = detect::Detect::current_os();

    let ver_dir = VersionDirectory {
        current_version: Version::parse("1.0.0-alpha2").unwrap(),
        nightly_version: Version::parse("1.0.0-alpha3").unwrap(),
    };
    let vinfo = if options.nightly {
        VersionInfo {
            package_suffix: package_suffix(&ver_dir.nightly_version),
            nightly: true,
            package_name: package_name(&ver_dir.nightly_version),
        }
    } else {
        VersionInfo {
            package_suffix: package_suffix(&ver_dir.current_version),
            nightly: false,
            package_name: package_name(&ver_dir.current_version),
        }
    };

    match &detect.os_info {
        detect::OsInfo::Linux(linux) => {
            let operations = match linux.get_distribution() {
                detect::linux::Distribution::Ubuntu(ubuntu) => {
                    ubuntu::prepare(options, &vinfo, &detect, linux, ubuntu)?
                }
                detect::linux::Distribution::Debian(debian) => {
                    debian::prepare(options, &vinfo, &detect, linux, debian)?
                }
                detect::linux::Distribution::Centos(centos) => {
                    centos::prepare(options, &vinfo, &detect, linux, centos)?
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
        _ => todo!(),
    }
}
