mod config;
mod exit_codes;
mod main;
mod platform;
pub mod local;
pub mod options;
pub mod repository;
pub mod ver;

pub mod macos;
pub mod linux;
pub mod windows;

mod control;
mod create;
mod destroy;
mod info;
mod install;
mod link;
mod list_versions;
mod reset_password;
mod revert;
mod status;
mod uninstall;
mod upgrade;
pub mod project;

pub use main::{instance_main, server_main, project_main};
