mod debian_like;
pub mod detect;
pub mod distribution;
mod main;
mod metadata;
pub mod methods;
pub mod options;
pub mod os_trait;
pub mod remote;
pub mod version;

// OSs
mod centos;
mod debian;
mod linux;
mod macos;
mod ubuntu;
mod unix;
mod unknown_os;
mod windows;

// Methods
mod docker;
pub mod package;

// commands
mod control;
pub mod destroy;
pub mod errors;
mod info;
pub mod init;
pub mod install;
mod list_versions;
mod reset_password;
mod revert;
mod status;
mod uninstall;
mod upgrade;

pub use main::main;

pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for c in chars {
        if !c.is_alphanumeric() && c != '_' {
            return false;
        }
    }
    return true;
}
