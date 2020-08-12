use serde::{Serialize, Deserialize};


#[derive(Serialize, Deserialize, Debug)]
pub struct Credentials {
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}
