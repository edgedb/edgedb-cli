#[cfg(feature="github_nightly")]
#[path="../docker.rs"]
mod docker;

#[cfg(feature="github_nightly")]
#[path="../measure.rs"]
mod measure;

#[cfg(feature="github_nightly")] mod common;

#[cfg(feature="github_nightly")] mod compat;
#[cfg(feature="github_nightly")] mod install;
#[cfg(feature="github_nightly")] mod upgrade;
#[cfg(feature="github_nightly")] mod project;
