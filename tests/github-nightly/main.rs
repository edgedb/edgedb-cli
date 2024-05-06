#![cfg(feature = "github_nightly")]

#[path = "../common/docker.rs"]
mod docker;

#[path = "../common/measure.rs"]
mod measure;

mod common;
mod compat;
mod install;
mod project;
mod upgrade;
