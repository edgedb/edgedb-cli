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

mod control;
mod create;
mod destroy;
mod info;
mod install;
mod list_versions;
mod revert;
mod status;
mod uninstall;
mod upgrade;
pub mod project;

pub use main::{instance_main, server_main, project_main};
