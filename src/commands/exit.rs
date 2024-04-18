use std::process::exit;

#[derive(Debug, thiserror::Error)]
#[error("Exit with status {}", _0)]
pub struct ExitCode(i32);

impl ExitCode {
    pub fn new(code: i32) -> ExitCode {
        ExitCode(code)
    }
    pub fn code(&self) -> i32 {
        self.0
    }
    pub fn exit(&self) -> ! {
        exit(self.code())
    }
}
