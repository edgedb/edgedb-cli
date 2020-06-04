#[cfg(target_env="musl")]
mod docker;
#[cfg(target_env="musl")]
mod certs;

// Tests are running under musl, because we need to upload a CLI binary to the
// docker container. The most universal way to do that is to build a static
// (musl) binary.
#[cfg(target_env="musl")]
mod ubuntu;
#[cfg(target_env="musl")]
mod debian;
#[cfg(target_env="musl")]
mod centos;
#[cfg(target_env="musl")]
mod self_install;

