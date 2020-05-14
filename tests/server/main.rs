mod docker;

// Tests are running under musl, because we need to upload a CLI binary to the
// docker container. The most universal way to do that is to build a static
// (musl) binary.
#[cfg(target_env="musl")]
mod ubuntu;
#[cfg(target_env="musl")]
mod debian;

