pub mod options;
mod main;
mod methods;
mod detect;
mod remote;
mod version;
mod os_trait;
mod debian_like;

// OSs
mod linux;
mod debian;
mod ubuntu;
mod centos;
mod macos;
mod windows;
mod unknown_os;

// Methods
mod docker;
mod package;

// commands
mod control;
mod init;
mod install;
mod list_versions;
mod status;
mod upgrade;

pub use main::main;
pub use control::get_instance;


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
