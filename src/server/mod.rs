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
mod install;
mod list_versions;
mod init;
mod control;
mod upgrade;

pub use main::main;
pub use control::get_instance;
