#[cfg(all(feature="github_nightly"))]
#[path="../docker.rs"]
mod docker;

#[cfg(all(feature="github_nightly"))]
mod upgrade;
