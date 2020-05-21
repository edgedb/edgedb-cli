pub mod options;
mod main;
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

pub use main::main;
