#![cfg(feature = "github_action_install")]

#[path = "../common/docker.rs"]
mod docker;

#[path = "../common/certs.rs"]
mod certs;

mod install;
