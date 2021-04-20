#[cfg(all(feature="github_nightly"))]
#[path="../docker.rs"]
mod docker;

#[cfg(all(feature="github_nightly"))]
#[path="../measure.rs"]
mod measure;

#[cfg(all(feature="github_nightly"))] mod common;

#[cfg(all(feature="github_nightly"))] mod compat;
#[cfg(all(feature="github_nightly"))] mod install;
#[cfg(all(feature="github_nightly"))] mod upgrade;
