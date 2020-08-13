use serde::{Serialize, Deserialize};


#[derive(Serialize, Deserialize, Debug)]
pub struct Credentials {
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub host: Option<String>,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}
