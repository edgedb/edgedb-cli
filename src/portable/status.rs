#[derive(Debug)]
pub enum Service {
    Running { pid: u32 },
    Failed { exit_code: Option<u16> },
    Inactive { error: String },
}
