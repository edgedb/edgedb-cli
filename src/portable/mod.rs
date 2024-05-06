pub mod config;
pub(crate) mod exit_codes;
pub mod local;
mod main;
pub mod options;
pub mod platform;
pub mod repository;
pub mod ver;

pub mod linux;
pub mod macos;
pub mod windows;

mod control;
mod create;
mod credentials;
mod destroy;
mod info;
pub mod install;
mod link;
mod list_versions;
pub mod project;
mod reset_password;
mod resize;
mod revert;
pub mod status;
mod uninstall;
mod upgrade;

pub use main::{instance_main, project_main, server_main};
pub use reset_password::password_hash;
