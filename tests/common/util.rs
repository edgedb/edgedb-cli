use assert_cmd::assert::Assert;

pub trait OutputExt {
    fn context(self, name: &'static str, description: &'static str) -> Self;
}

impl OutputExt for Assert {
    fn context(mut self, name: &'static str, description: &'static str) -> Self {
        self = self.append_context(name, description);
        let out = self.get_output();
        println!("------ {}: {} (STDOUT) -----", name, description);
        println!("{}", String::from_utf8_lossy(&out.stdout));
        println!("------ {}: {} (STDERR) -----", name, description);
        println!("{}", String::from_utf8_lossy(&out.stderr));
        self
    }
}
