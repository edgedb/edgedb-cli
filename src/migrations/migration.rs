#[derive(Debug)]
pub struct Migration {
    pub message: Option<String>,
    pub id: String,
    pub parent_id: String,
}
