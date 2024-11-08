#![allow(unused)]

use assert_cmd::assert::Assert;

use const_format::concatcp;

#[path = "../../src/branding.rs"]
mod branding;

pub const EXPECTED_VERSION: &str =
    concatcp!(branding::BRANDING_CLI, " ", env!("CARGO_PKG_VERSION"));

pub trait OutputExt {
    fn context(self, name: &'static str, description: &'static str) -> Self;
}

impl OutputExt for Assert {
    fn context(mut self, name: &'static str, description: &'static str) -> Self {
        self = self.append_context(name, description);
        let out = self.get_output();
        println!("------ {name}: {description} (STDOUT) -----");
        println!("{}", String::from_utf8_lossy(&out.stdout));
        println!("------ {name}: {description} (STDERR) -----");
        println!("{}", String::from_utf8_lossy(&out.stderr));
        self
    }
}
