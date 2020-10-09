#[cfg(all(feature="github_nightly"))]
#[path="../docker.rs"]
mod docker;

#[cfg(all(feature="github_nightly"))] mod common;

#[cfg(all(feature="github_nightly"))] mod install;
#[cfg(all(feature="github_nightly"))] mod upgrade;
