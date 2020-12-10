pub mod options;
mod main;
mod metadata;
mod methods;
mod detect;
mod distribution;
pub mod remote;
pub mod version;
mod os_trait;
mod debian_like;

// OSs
mod unix;
mod linux;
mod debian;
mod ubuntu;
mod centos;
mod macos;
mod windows;
mod unknown_os;

// Methods
mod docker;
pub mod package;

// commands
mod errors;
mod control;
mod destroy;
mod info;
mod init;
mod install;
mod uninstall;
mod list_versions;
mod reset_password;
mod status;
mod upgrade;

pub use main::main;


fn is_valid_name(name: &str) -> bool {
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
    return true
}
