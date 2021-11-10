mod config;
mod exit_codes;
mod local;
mod main;
mod platform;
mod repository;
mod ver;

mod macos;
mod linux;
mod windows;

mod create;
mod destroy;
pub mod project;
mod install;
mod list_versions;
mod status;

pub use main::{instance_main, server_main, project_main};
