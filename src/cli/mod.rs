pub mod directory_check;
pub mod env;
pub mod install;
pub mod logo;
pub mod main;
pub mod migrate;
pub mod options;
pub mod upgrade;

#[macro_use]
mod markdown;

pub use main::main;
