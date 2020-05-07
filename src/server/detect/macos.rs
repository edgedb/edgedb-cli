use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct OsInfo {
}

impl OsInfo {
    pub fn new() -> OsInfo {
        OsInfo {
        }
    }
    pub fn detect_all(&self) {
    }
}
