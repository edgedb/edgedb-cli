#[derive(Debug)]
pub struct Migration {
    pub message: Option<String>,
    pub id: Option<String>,
    pub parent_id: Option<String>,
}
