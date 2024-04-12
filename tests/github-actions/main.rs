#[cfg(feature="github_action_install")]
#[path="../docker.rs"]
mod docker;

#[cfg(feature="github_action_install")]
#[path="../certs.rs"]
mod certs;

#[cfg(feature="github_action_install")]
mod install;
