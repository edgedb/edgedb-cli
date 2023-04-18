pub(crate) mod exit_codes;
mod main;
pub mod config;
pub mod local;
pub mod options;
pub mod platform;
pub mod repository;
pub mod ver;

pub mod macos;
pub mod linux;
pub mod windows;

mod control;
mod create;
mod credentials;
mod destroy;
mod info;
pub mod install;
mod link;
mod list_versions;
mod reset_password;
mod revert;
pub mod status;
mod uninstall;
mod upgrade;
pub mod project;

pub use main::{instance_main, server_main, project_main};
