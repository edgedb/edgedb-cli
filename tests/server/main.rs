#[cfg(target_env = "musl")]
#[path = "../certs.rs"]
mod certs;
#[cfg(target_env = "musl")]
#[path = "../docker.rs"]
mod docker;

// Tests are running under musl, because we need to upload a CLI binary to the
// docker container. The most universal way to do that is to build a static
// (musl) binary.
#[cfg(target_env = "musl")]
mod centos;
#[cfg(target_env = "musl")]
mod debian;
#[cfg(target_env = "musl")]
mod self_install;
#[cfg(target_env = "musl")]
mod ubuntu;
